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
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 14)

            Divider()
                .overlay(HexTheme.borderWeak)

            KeyboardSurfaceView(model: viewModel)
                .frame(height: 140)
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
        viewModel.needsGlobeKey = true

        switch scene {
        case "keyboard-cold":
            viewModel.configureFixture(
                snapshot: .offline,
                now: now,
                style: .polished
            )

        case "keyboard-no-access":
            viewModel.configureFixture(
                snapshot: .offline,
                now: now,
                hasFullAccess: false
            )

        case "keyboard-warm":
            viewModel.configureFixture(
                snapshot: KeyboardSnapshot(
                    state: .ready,
                    heartbeat: now - 0.1,
                    expiresAt: now + 14 * 60 + 20
                ),
                now: now,
                style: .casual
            )

        case "keyboard-recording":
            viewModel.configureFixture(
                snapshot: KeyboardSnapshot(
                    state: .recording,
                    heartbeat: now - 0.05,
                    expiresAt: now + 15 * 60,
                    recordingStartedAt: now - 5,
                    audioLevel: 0.78,
                    loudnessDB: -31.0,
                    jobID: "snap-job"
                ),
                now: now,
                style: .polished
            )

        case "keyboard-transcribing":
            viewModel.configureFixture(
                snapshot: KeyboardSnapshot(
                    state: .transcribing,
                    heartbeat: now - 0.05,
                    expiresAt: now + 15 * 60,
                    jobID: "snap-job"
                ),
                now: now,
                style: .polished
            )

        case "keyboard-style":
            viewModel.configureFixture(
                snapshot: KeyboardSnapshot(
                    state: .ready,
                    heartbeat: now - 0.1,
                    expiresAt: now + 14 * 60
                ),
                now: now,
                style: .casual,
                activeTray: .style
            )

        default:
            break
        }
    }
}

/// Hosts a clean conversation thread and an auto-focused real `UITextField`
/// so iOS Simulator docks the actual `HEXKeyboard.appex` `UIInputViewController`
/// extension process at the bottom of the screen.
struct HostedKeyboardConversationStage: View {
    let scene: String
    @Bindable var dictation: DictationController
    @State private var draft = HostedFieldSpec.current.initialDraft

    static var requestedScene: String? {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else {
            return nil
        }
        let value = args[idx + 1]
        return value.hasPrefix("hosted-keyboard") ? value : nil
    }

    private var spec: HostedFieldSpec { .current }

    private var usesLightAppearance: Bool {
        if let forceLight = spec.isLightApp { return forceLight }
        return scene.hasSuffix("-light") || usesDarkField
    }

    /// A light app whose field asks for a dark keyboard.
    private var usesDarkField: Bool {
        spec.darkKeyboard || scene.hasSuffix("-dark-field")
    }

    var body: some View {
        let isLight = usesLightAppearance
        let canvas = isLight
            ? Color(red: 0xF5 / 255.0, green: 0xF6 / 255.0, blue: 0xF8 / 255.0)
            : Color(red: 0x09 / 255.0, green: 0x0A / 255.0, blue: 0x0C / 255.0)
        let incomingFill = isLight
            ? Color(red: 0xE6 / 255.0, green: 0xE8 / 255.0, blue: 0xEE / 255.0)
            : Color(red: 0x1A / 255.0, green: 0x1C / 255.0, blue: 0x21 / 255.0)
        let outgoingFill = Color(red: 0x2A / 255.0, green: 0x66 / 255.0, blue: 0xF5 / 255.0)
        let primaryInk = isLight
            ? Color(red: 0x12 / 255.0, green: 0x14 / 255.0, blue: 0x19 / 255.0)
            : HexTheme.textBase
        let secondaryInk = isLight
            ? Color(red: 0x5A / 255.0, green: 0x60 / 255.0, blue: 0x6E / 255.0)
            : HexTheme.textMuted
        let fieldFill = isLight
            ? Color.white
            : Color(red: 0x13 / 255.0, green: 0x15 / 255.0, blue: 0x19 / 255.0)
        let fieldStroke = isLight
            ? Color.black.opacity(0.08)
            : Color.white.opacity(0.08)

        VStack(spacing: 0) {
            // Conversation header
            HStack {
                Image(systemName: "chevron.left")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(outgoingFill)
                Spacer()
                VStack(spacing: 2) {
                    Circle()
                        .fill(incomingFill)
                        .frame(width: 30, height: 30)
                        .overlay(
                            Text("A")
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(primaryInk)
                        )
                    Text("Alex")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(primaryInk)
                }
                Spacer()
                Image(systemName: "video")
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(outgoingFill)
            }
            .padding(.horizontal, 18)
            .padding(.top, 8)
            .padding(.bottom, 12)

            Divider()
                .overlay(fieldStroke)

            Spacer(minLength: 12)

            // Thread bubbles
            VStack(spacing: 10) {
                HStack {
                    Text("When are we reviewing the updated iOS keyboard build?")
                        .font(.system(size: 15))
                        .foregroundStyle(primaryInk)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .background(incomingFill, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                    Spacer(minLength: 56)
                }

                HStack {
                    Spacer(minLength: 56)
                    Text("Testing the full-width Dot Scope bar right now.")
                        .font(.system(size: 15))
                        .foregroundStyle(Color.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .background(outgoingFill, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                }

                HStack {
                    Text("Nice — send the time once it's ready.")
                        .font(.system(size: 15))
                        .foregroundStyle(primaryInk)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .background(incomingFill, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                    Spacer(minLength: 56)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 14)

            // Real UITextField composer bar above the docked UIInputViewController keyboard
            HStack(spacing: 10) {
                Image(systemName: "plus")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(secondaryInk)
                    .frame(width: 32, height: 32)
                    .background(incomingFill, in: Circle())

                PlainKeyboardHostTextField(
                    text: $draft,
                    placeholder: "Message",
                    isLight: isLight,
                    darkKeyboard: usesDarkField,
                    keyboardType: spec.keyboardType,
                    returnKeyType: spec.returnKeyType
                )
                .frame(height: 36)
                .padding(.horizontal, 13)
                .background(fieldFill, in: Capsule())
                .overlay(Capsule().stroke(fieldStroke, lineWidth: 0.75))
            }
            .padding(.horizontal, 14)
            .padding(.bottom, 10)
        }
        .background(canvas.ignoresSafeArea())
        .preferredColorScheme(isLight ? .light : .dark)
        .onAppear {
            if let windowScene = UIApplication.shared.connectedScenes.first as? UIWindowScene {
                let mask: UIInterfaceOrientationMask = spec.isLandscape ? .landscapeRight : .portrait
                windowScene.requestGeometryUpdate(.iOS(interfaceOrientations: mask))
            }
        }
    }
}

private struct HostedFieldSpec {
    let initialDraft: String
    let isLightApp: Bool?
    let darkKeyboard: Bool
    let keyboardType: UIKeyboardType
    let returnKeyType: UIReturnKeyType
    var isLandscape = false

    static var current: HostedFieldSpec {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--hosted-field"), idx + 1 < args.count else {
            return HostedFieldSpec(initialDraft: "", isLightApp: nil, darkKeyboard: false, keyboardType: .default, returnKeyType: .send)
        }
        switch args[idx + 1] {
        case "letters-dark":
            return HostedFieldSpec(initialDraft: "Running 10 min late, ", isLightApp: false, darkKeyboard: false, keyboardType: .default, returnKeyType: .default)
        case "letters-light":
            return HostedFieldSpec(initialDraft: "Running 10 min late, ", isLightApp: true, darkKeyboard: false, keyboardType: .default, returnKeyType: .default)
        case "dark-field":
            return HostedFieldSpec(initialDraft: "Running 10 min late, ", isLightApp: true, darkKeyboard: true, keyboardType: .default, returnKeyType: .default)
        case "shift-dark":
            return HostedFieldSpec(initialDraft: "Running 10 min late. ", isLightApp: false, darkKeyboard: false, keyboardType: .default, returnKeyType: .default)
        case "shift-light":
            return HostedFieldSpec(initialDraft: "Running 10 min late. ", isLightApp: true, darkKeyboard: false, keyboardType: .default, returnKeyType: .default)
        case "typing-dark":
            return HostedFieldSpec(initialDraft: "Running 10 min lat", isLightApp: false, darkKeyboard: false, keyboardType: .default, returnKeyType: .send)
        case "typing-light":
            return HostedFieldSpec(initialDraft: "Running 10 min lat", isLightApp: true, darkKeyboard: false, keyboardType: .default, returnKeyType: .send)
        case "typo-dark":
            return HostedFieldSpec(initialDraft: "Running 10 min teh", isLightApp: false, darkKeyboard: false, keyboardType: .default, returnKeyType: .send)
        case "numbers-dark":
            return HostedFieldSpec(initialDraft: "Running 10 min late, home by 7:3", isLightApp: false, darkKeyboard: false, keyboardType: .numbersAndPunctuation, returnKeyType: .default)
        case "numbers-light":
            return HostedFieldSpec(initialDraft: "Running 10 min late, home by 7:3", isLightApp: true, darkKeyboard: false, keyboardType: .numbersAndPunctuation, returnKeyType: .default)
        case "email-dark":
            return HostedFieldSpec(initialDraft: "alex@", isLightApp: false, darkKeyboard: false, keyboardType: .emailAddress, returnKeyType: .done)
        case "url-dark":
            return HostedFieldSpec(initialDraft: "https://anoma.ly/", isLightApp: false, darkKeyboard: false, keyboardType: .URL, returnKeyType: .go)
        case "recording-dark":
            return HostedFieldSpec(initialDraft: "Running 10 min late, ", isLightApp: false, darkKeyboard: false, keyboardType: .default, returnKeyType: .default)
        case "recording-light":
            return HostedFieldSpec(initialDraft: "Running 10 min late, ", isLightApp: true, darkKeyboard: false, keyboardType: .default, returnKeyType: .default)
        case "landscape-dark":
            return HostedFieldSpec(initialDraft: "Running 10 min late. ", isLightApp: false, darkKeyboard: false, keyboardType: .default, returnKeyType: .default, isLandscape: true)
        case "landscape-light":
            return HostedFieldSpec(initialDraft: "Running 10 min late. ", isLightApp: true, darkKeyboard: false, keyboardType: .default, returnKeyType: .default, isLandscape: true)
        default:
            return HostedFieldSpec(initialDraft: "", isLightApp: nil, darkKeyboard: false, keyboardType: .default, returnKeyType: .send)
        }
    }
}

private struct PlainKeyboardHostTextField: UIViewRepresentable {
    @Binding var text: String
    let placeholder: String
    let isLight: Bool
    var darkKeyboard = false
    var keyboardType: UIKeyboardType = .default
    var returnKeyType: UIReturnKeyType = .send

    final class Coordinator: NSObject, UITextFieldDelegate {
        var parent: PlainKeyboardHostTextField
        init(parent: PlainKeyboardHostTextField) { self.parent = parent }

        @objc func editingChanged(_ sender: UITextField) {
            parent.text = sender.text ?? ""
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    func makeUIView(context: Context) -> UITextField {
        let tf = UITextField()
        tf.delegate = context.coordinator
        tf.addTarget(
            context.coordinator,
            action: #selector(Coordinator.editingChanged(_:)),
            for: .editingChanged
        )
        tf.font = .systemFont(ofSize: 15)
        tf.autocorrectionType = .default
        tf.spellCheckingType = .no
        tf.smartInsertDeleteType = .no
        tf.keyboardType = keyboardType
        tf.returnKeyType = returnKeyType
        tf.text = text
        tf.keyboardAppearance = isLight && !darkKeyboard ? .light : .dark
        tf.inputAssistantItem.leadingBarButtonGroups = []
        tf.inputAssistantItem.trailingBarButtonGroups = []
        tf.attributedPlaceholder = NSAttributedString(
            string: placeholder,
            attributes: [
                .foregroundColor: isLight
                    ? UIColor(white: 0.50, alpha: 1)
                    : UIColor(white: 0.45, alpha: 1),
            ]
        )
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
            tf.becomeFirstResponder()
        }
        return tf
    }

    func updateUIView(_ uiView: UITextField, context: Context) {
        context.coordinator.parent = self
        uiView.textColor = isLight
            ? UIColor(red: 0x12 / 255.0, green: 0x14 / 255.0, blue: 0x19 / 255.0, alpha: 1)
            : UIColor(white: 0.95, alpha: 1)
        uiView.keyboardAppearance = isLight && !darkKeyboard ? .light : .dark
        if uiView.text != text {
            uiView.text = text
        }
    }
}
