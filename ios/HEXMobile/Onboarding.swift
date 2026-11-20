import SwiftUI
import UIKit

struct OnboardingView: View {
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let dictation: DictationController
    let monitor: KeyboardSetupMonitor
    @Binding var page: OnboardingPage?
    @State private var forward = true
    @State private var tryText = OnboardingFixture.current == .tryDone ? "See you at ten." : ""
    @State private var fixtureModelProgress: Double? = OnboardingFixture.current == .allow ? 0.42 : nil

    var body: some View {
        ZStack {
            switch page {
            case .welcome?:
                OnboardingWelcomePage { go(to: .allow) }
                    .transition(pageTransition)
            case .allow?:
                OnboardingAllowPage(
                    dictation: dictation,
                    monitor: monitor,
                    fixtureModelProgress: fixtureModelProgress,
                    onBack: { go(to: .welcome) },
                    onContinue: { go(to: .tryKeyboard) }
                )
                .transition(pageTransition)
            case .tryKeyboard?:
                OnboardingTryPage(
                    monitor: monitor,
                    text: $tryText,
                    onBack: { go(to: .allow) },
                    onFinish: finish
                )
                .transition(pageTransition)
            case nil:
                EmptyView()
            }
        }
        .frame(maxWidth: 500)
        .padding(.horizontal, 20)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task {
            while !Task.isCancelled {
                monitor.refresh()
                try? await Task.sleep(for: .seconds(1))
            }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active {
                monitor.refresh()
            }
        }
        .task { await runTour() }
    }

    /// `onboarding-tour`: steps through every page on the fixture timeline.
    private func runTour() async {
        guard OnboardingFixture.current == .tour else { return }
        typealias Tour = OnboardingFixture.Tour
        if Tour.start == nil {
            Tour.start = Date().addingTimeInterval(Tour.leadIn)
        }
        while !Task.isCancelled {
            let t = Tour.elapsed
            monitor.refresh()
            if t < Tour.modelReady {
                fixtureModelProgress = 0.08 + 0.92 * max(0, t - Tour.toAllow) / (Tour.modelReady - Tour.toAllow)
            } else {
                fixtureModelProgress = nil
            }
            if t >= Tour.done {
                finish()
                return
            } else if t >= Tour.text, tryText.isEmpty {
                tryText = "See you at ten."
            } else if t >= Tour.toTry, page == .allow {
                go(to: .tryKeyboard)
            } else if t >= Tour.toAllow, page == .welcome {
                go(to: .allow)
            }
            try? await Task.sleep(for: .milliseconds(33))
        }
    }

    private var pageTransition: AnyTransition {
        guard !reduceMotion else { return .opacity }
        let shift: CGFloat = forward ? 28 : -28
        return .asymmetric(
            insertion: .opacity.combined(with: .offset(x: shift)),
            removal: .opacity.combined(with: .offset(x: -shift))
        )
    }

    private func go(to next: OnboardingPage) {
        forward = next.number > (page?.number ?? 0)
        OnboardingProgress.save(next)
        withAnimation(HexMotion.stageAnimation(reduceMotion: reduceMotion)) {
            page = next
        }
    }

    private func finish() {
        OnboardingProgress.complete()
        withAnimation(HexMotion.settleAnimation(reduceMotion: reduceMotion)) {
            page = nil
        }
    }
}

// MARK: - Welcome

private struct OnboardingWelcomePage: View {
    let onStart: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            GeometryReader { geo in
                ScrollView {
                    VStack(spacing: 0) {
                        HexMarkView(size: 64)
                            .padding(.bottom, 20)

                        Label("100% on device", systemImage: "lock.fill")
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(HexTheme.textBase)
                            .padding(.horizontal, 12)
                            .frame(height: 28)
                            .background(HexTheme.bgBase, in: Capsule())
                            .padding(.bottom, 16)

                        Text("Talk instead of type,\nin any app.")
                            .font(.system(size: 28, weight: .semibold))
                            .tracking(-0.4)
                            .foregroundStyle(HexTheme.textBase)
                            .multilineTextAlignment(.center)
                            .fixedSize(horizontal: false, vertical: true)
                            .padding(.bottom, 36)

                        VStack(alignment: .leading, spacing: 22) {
                            point(icon: "keyboard", title: "Dictate right in the keyboard")
                            point(icon: "iphone", title: "Free, and nothing leaves your iPhone")
                            point(icon: "music.note", title: "Your music keeps playing")
                        }
                        .frame(maxWidth: 360, alignment: .leading)
                    }
                    .padding(.vertical, 24)
                    .frame(maxWidth: .infinity, minHeight: geo.size.height)
                }
                .scrollBounceBehavior(.basedOnSize)
            }

            OnboardingPrimaryButton(title: "Get started", action: onStart)
                .padding(.top, 12)
                .padding(.bottom, 12)
        }
    }

    private func point(icon: String, title: LocalizedStringKey) -> some View {
        HStack(alignment: .center, spacing: 14) {
            Image(systemName: icon)
                .font(.system(size: 16, weight: .medium))
                .foregroundStyle(HexTheme.textBase)
                .frame(width: 38, height: 38)
                .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: 11, style: .continuous))

            Text(title)
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(HexTheme.textBase)
        }
        .accessibilityElement(children: .combine)
    }
}

// MARK: - Allow

private struct OnboardingAllowPage: View {
    @Environment(\.openURL) private var openURL
    let dictation: DictationController
    let monitor: KeyboardSetupMonitor
    let fixtureModelProgress: Double?
    let onBack: () -> Void
    let onContinue: () -> Void
    @State private var showsKeyboardFallback = OnboardingFixture.current == .allow && ProcessInfo.processInfo.arguments.contains("--onboarding-fallback")

    /// Full Access can't be verified from the app; the next page checks it. The speech model
    /// keeps downloading in the background and never holds up Continue, and neither does the
    /// microphone: someone who declines it can still type with Hex and allow it later
    /// (App Review 5.1.1(iv)).
    private var canContinue: Bool {
        monitor.keyboardAdded != false
    }

    /// Before iOS's microphone prompt, Hex's own buttons say Continue or Open Settings, never
    /// Allow (App Review 5.1.1(iv)).
    private func continueTapped() {
        if monitor.microphone == .undetermined {
            monitor.requestMicrophone { onContinue() }
        } else {
            onContinue()
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            OnboardingHeader(page: .allow, onBack: onBack)

            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    Text("Set up Hex")
                        .font(.system(size: 28, weight: .semibold))
                        .tracking(-0.4)
                        .foregroundStyle(HexTheme.textBase)

                    // Microphone first: iOS lists Microphone on Hex's Settings page only once
                    // the app has asked, so asking first means one trip to Settings.
                    VStack(spacing: 0) {
                        OnboardingCheckRow(
                            title: "Microphone",
                            detail: monitor.microphone == .granted ? nil : "So Hex can hear you dictate",
                            done: monitor.microphone == .granted
                        ) {
                            if monitor.microphone == .denied {
                                OnboardingSmallButton(title: "Open Settings") { openSettings(openURL) }
                            }
                        }
                        Rectangle().fill(HexTheme.borderWeak).frame(height: 0.5)
                        OnboardingCheckRow(title: "Keyboard", detail: "Add Hex and Allow Full Access", done: monitor.keyboardAdded == true) {
                            if !monitor.fullAccessOn {
                                OnboardingSmallButton(title: "Open Settings") {
                                    if monitor.microphone == .undetermined {
                                        monitor.requestMicrophone { openSettings(openURL) }
                                    } else {
                                        openSettings(openURL)
                                    }
                                }
                            }
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 4)
                    .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))

                    VStack(alignment: .leading, spacing: 8) {
                        Text("Keyboards  ›  Hex Keyboard  ›  Allow Full Access")
                        Button {
                            withAnimation(HexMotion.reducedFade) { showsKeyboardFallback.toggle() }
                        } label: {
                            Text("Don't see Keyboards?")
                                .underline()
                        }
                        .buttonStyle(.plain)
                        if showsKeyboardFallback {
                            Text("General  ›  Keyboard  ›  Keyboards  ›  Add New Keyboard  ›  Hex")
                                .transition(.opacity)
                        }
                    }
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
                    .padding(.horizontal, 4)
                }
                .padding(.top, 8)
                .padding(.bottom, 16)
            }
            .scrollBounceBehavior(.basedOnSize)

            OnboardingPrimaryButton(title: "Continue", enabled: canContinue, action: continueTapped)
                .padding(.top, 8)
                .padding(.bottom, 12)
        }
    }
}

// MARK: - Try the keyboard

private struct OnboardingTryPage: View {
    @Environment(\.openURL) private var openURL
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let monitor: KeyboardSetupMonitor
    @Binding var text: String
    let onBack: () -> Void
    let onFinish: () -> Void

    @State private var keyboardActivity = 0
    @State private var lastSwitchAt: Date?
    @State private var keyboardCameUpAt = Date.distantPast
    @State private var fullAccessLooksOff = OnboardingFixture.current == .tryOff
    @FocusState private var fieldFocused: Bool

    /// How long the keyboard gets to report Full Access after a switch or new text.
    private static let fullAccessGrace: Duration = .seconds(4)
    /// The keyboard coming up (focus, or back from Settings) also posts an input mode change.
    private static let keyboardSettle: TimeInterval = 1.5

    private var hasText: Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var complete: Bool {
        monitor.fullAccessOn && hasText
    }

    var body: some View {
        VStack(spacing: 0) {
            OnboardingHeader(page: .tryKeyboard, onBack: onBack)

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    HStack(alignment: .center, spacing: 10) {
                        if complete {
                            Image(systemName: "checkmark.circle.fill")
                                .font(.system(size: 24, weight: .medium))
                                .foregroundStyle(HexTheme.success)
                                .transition(.opacity.combined(with: .scale(scale: 0.9)))
                        }
                        Text(complete ? "You're all set" : "Try the keyboard")
                            .font(.system(size: 28, weight: .semibold))
                            .tracking(-0.4)
                            .foregroundStyle(HexTheme.textBase)
                            .contentTransition(.opacity)
                    }

                    Group {
                        if complete {
                            Text("Tap the Hex key in any app to talk.")
                        } else {
                            Text("Switch to Hex with \(Image(systemName: "globe")), then tap the Hex key.")
                        }
                    }
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, -8)

                    TextField("Your words show up here", text: $text, axis: .vertical)
                        .font(.system(size: 17))
                        .foregroundStyle(HexTheme.textBase)
                        .focused($fieldFocused)
                        .lineLimit(2...5)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 14)
                        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))

                    if fullAccessLooksOff && !monitor.fullAccessOn {
                        fullAccessNote
                            .transition(.opacity.combined(with: .scale(scale: 0.98)))
                    }
                }
                .padding(.top, 8)
                .padding(.bottom, 16)
            }
            .scrollBounceBehavior(.basedOnSize)

            ZStack {
                Button("Skip for now", action: onFinish)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(HexTheme.textMuted)
                    .frame(maxWidth: .infinity, minHeight: 50)
                    .contentShape(Rectangle())
                    .buttonStyle(.plain)
                    .hexControlReveal(!complete, reduceMotion: reduceMotion, scale: 0.96)

                OnboardingPrimaryButton(title: "Done", action: onFinish)
                    .hexControlReveal(complete, reduceMotion: reduceMotion, scale: 0.96)
            }
            .padding(.top, 8)
            .padding(.bottom, 12)
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: complete)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: fullAccessLooksOff)
        .onAppear {
            guard OnboardingFixture.current != .tryDone else { return }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
                fieldFocused = true
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: UITextInputMode.currentInputModeDidChangeNotification)) { _ in
            lastSwitchAt = Date()
            noteKeyboardActivity()
        }
        .onChange(of: fieldFocused) { _, focused in
            if focused { keyboardCameUpAt = Date() }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { keyboardCameUpAt = Date() }
        }
        .onChange(of: text) { _, _ in
            if hasText { noteKeyboardActivity() }
        }
        .onChange(of: monitor.fullAccessOn) { _, on in
            if on { fullAccessLooksOff = false }
        }
        .onChange(of: complete) { _, done in
            if done { fieldFocused = false }
        }
        .task(id: keyboardActivity) {
            // iOS tells only the keyboard about Full Access, and the keyboard writes it to the
            // App Group as soon as it opens with it; nothing a few seconds after a keyboard
            // switch or new text means it's off (or the Hex keyboard isn't the one up).
            guard keyboardActivity > 0 else { return }
            try? await Task.sleep(for: Self.fullAccessGrace)
            guard !Task.isCancelled, !monitor.fullAccessOn else { return }
            if !hasText, let switchedAt = lastSwitchAt,
               abs(switchedAt.timeIntervalSince(keyboardCameUpAt)) < Self.keyboardSettle {
                return
            }
            fullAccessLooksOff = true
        }
    }

    private func noteKeyboardActivity() {
        guard !monitor.fullAccessOn else { return }
        keyboardActivity += 1
    }

    private var fullAccessNote: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "exclamationmark.circle.fill")
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(HexTheme.warmMic)
                    .padding(.top, 1)
                Text("Turn on Allow Full Access")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
            }
            .accessibilityElement(children: .combine)

            OnboardingSecondaryButton(title: "Open Settings", icon: "gearshape.fill") {
                openSettings(openURL)
            }
        }
        .padding(16)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
    }
}

// MARK: - Pieces

@MainActor
private func openSettings(_ openURL: OpenURLAction) {
    if let url = URL(string: UIApplication.openSettingsURLString) {
        openURL(url)
    }
}

private struct OnboardingHeader: View {
    let page: OnboardingPage
    let onBack: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            Button(action: onBack) {
                Image(systemName: "chevron.left")
                    .font(.system(size: 17, weight: .medium))
                    .foregroundStyle(HexTheme.textMuted)
                    .frame(width: 44, height: 44, alignment: .leading)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Back")

            Spacer()

            Text("\(page.number) of \(OnboardingPage.allCases.count)")
                .font(.system(size: 13, weight: .medium))
                .monospacedDigit()
                .foregroundStyle(HexTheme.textFaint)
        }
        .frame(height: 44)
    }
}

private struct OnboardingCheckRow<Trailing: View>: View {
    let title: LocalizedStringKey
    var detail: LocalizedStringKey? = nil
    let done: Bool
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: done ? "checkmark.circle.fill" : "circle")
                .font(.system(size: 18, weight: .medium))
                .foregroundStyle(done ? HexTheme.success : HexTheme.textFaint)
                .contentTransition(.symbolEffect(.replace))
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 16))
                    .foregroundStyle(HexTheme.textBase)
                if let detail {
                    Text(detail)
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }
            Spacer(minLength: 8)
            trailing
        }
        .frame(minHeight: 52)
        .accessibilityElement(children: .contain)
    }
}

/// The one filled action on a page.
private struct OnboardingPrimaryButton: View {
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

private struct OnboardingSecondaryButton: View {
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

private struct OnboardingSmallButton: View {
    let title: LocalizedStringKey
    let action: () -> Void

    var body: some View {
        Button(title, action: action)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(HexTheme.textBase)
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(Color.white.opacity(0.09), in: Capsule())
            .buttonStyle(OnboardingPressStyle())
    }
}

private struct OnboardingPressStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}