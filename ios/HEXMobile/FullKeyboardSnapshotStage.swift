import SwiftUI
import UIKit

/// Renders the experimental `FullKeyboardView` under a composer when
/// `--snap-state full-keyboard-<state>[-dark|-dark-field]` is passed on Simulator.
/// `-dark` is a dark app; `-dark-field` is a light app whose field asks for a dark keyboard.
struct FullKeyboardSnapshotStage: View {
    let scene: String

    static var requestedScene: String? {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else {
            return nil
        }
        let value = args[idx + 1]
        return value.hasPrefix("full-keyboard-") ? value : nil
    }

    private var darkField: Bool { scene.hasSuffix("-dark-field") }
    private var darkApp: Bool { scene.hasSuffix("-dark") }

    private var state: String {
        var name = String(scene.dropFirst("full-keyboard-".count))
        for suffix in ["-dark-field", "-dark"] where name.hasSuffix(suffix) {
            name = String(name.dropLast(suffix.count))
        }
        return name
    }

    private var draft: String {
        switch state {
        case "inserted": "Running 10 min late, should be home by 7:30. Start without me."
        case "numbers": "Running 10 min late, home by 7:3"
        case "shift": "Running 10 min late. "
        default: "Running 10 min late, "
        }
    }

    var body: some View {
        let ink = darkApp ? HexTheme.textBase : Color(red: 0x14 / 255, green: 0x16 / 255, blue: 0x1B / 255)
        let canvas = darkApp
            ? Color(red: 0x09 / 255, green: 0x0A / 255, blue: 0x0C / 255)
            : Color(red: 0xF5 / 255, green: 0xF6 / 255, blue: 0xF8 / 255)
        let backdrop = FullKeyboardPalette(isDark: darkApp || darkField).backdrop
        VStack(spacing: 0) {
            Spacer()
            Text(draft)
                .font(.system(size: 15))
                .foregroundStyle(ink)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(
                    darkApp ? Color(red: 0x13 / 255, green: 0x15 / 255, blue: 0x19 / 255) : .white,
                    in: Capsule()
                )
                .padding(.horizontal, 14)
                .padding(.bottom, 10)
            FullKeyboardFixture(state: state, draft: draft, darkKeyboard: darkApp || darkField)
                .frame(height: KeyboardLayout.height(for: .portrait))
                .background(Color(uiColor: backdrop).ignoresSafeArea(edges: .bottom))
        }
        .background(canvas.ignoresSafeArea())
        .preferredColorScheme(darkApp ? .dark : .light)
    }
}

private struct FullKeyboardFixture: UIViewRepresentable {
    let state: String
    let draft: String
    let darkKeyboard: Bool

    @MainActor
    final class Coordinator {
        let model = KeyboardViewModel()
        let target: FixtureTextTarget

        init(draft: String, darkKeyboard: Bool) {
            target = FixtureTextTarget(text: draft, appearance: darkKeyboard ? .dark : .light)
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(draft: draft, darkKeyboard: darkKeyboard)
    }

    func makeUIView(context: Context) -> FullKeyboardView {
        let model = context.coordinator.model
        model.needsGlobeKey = false
        let now: TimeInterval = 10_000
        let warm = KeyboardSnapshot(state: .ready, heartbeat: now - 0.1, expiresAt: now + 14 * 60)
        switch state {
        case "recording":
            model.configureFixture(
                snapshot: KeyboardSnapshot(
                    state: .recording,
                    heartbeat: now - 0.05,
                    expiresAt: now + 15 * 60,
                    recordingStartedAt: now - 5,
                    audioLevel: 0.78,
                    loudnessDB: -31,
                    jobID: "snap-job"
                ),
                now: now
            )
        case "transcribing":
            model.configureFixture(
                snapshot: KeyboardSnapshot(state: .transcribing, heartbeat: now - 0.05, expiresAt: now + 15 * 60, jobID: "snap-job"),
                now: now
            )
        case "cold":
            model.configureFixture(snapshot: .offline, now: now)
        case "no-access":
            model.configureFixture(snapshot: .offline, now: now, hasFullAccess: false)
        default:
            model.configureFixture(snapshot: warm, now: now)
        }

        let view = FullKeyboardView(model: model, target: context.coordinator.target)
        view.appearanceOverride = darkKeyboard
        view.orientationOverride = .portrait
        view.drawsBackdrop = true
        view.prepareForField()

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            switch state {
            case "pressed":
                view.debugPress("g")
            case "numbers":
                view.engine.touchBegan(id: 900, at: center(of: .layerSwitch(.numbers), in: view))
                view.engine.touchEnded(id: 900, at: center(of: .layerSwitch(.numbers), in: view))
                view.debugPress("3")
            case "symbols":
                for kind in [KeyKind.layerSwitch(.numbers), .layerSwitch(.symbols)] {
                    view.engine.touchBegan(id: 901, at: center(of: kind, in: view))
                    view.engine.touchEnded(id: 901, at: center(of: kind, in: view))
                }
            case "capslock":
                view.engine.touchBegan(id: 902, at: center(of: .shift, in: view))
                view.engine.touchEnded(id: 902, at: center(of: .shift, in: view))
                view.engine.touchBegan(id: 903, at: center(of: .shift, in: view))
                view.engine.touchEnded(id: 903, at: center(of: .shift, in: view))
            case "inserted":
                view.didInsertDictation("should be home by 7:30. Start without me.")
            case "style":
                view.debugShowStyleMenu()
            default:
                break
            }
        }
        return view
    }

    func updateUIView(_ uiView: FullKeyboardView, context: Context) {}
}

@MainActor
private func center(of kind: KeyKind, in view: FullKeyboardView) -> CGPoint {
    guard let key = view.engine.layout.key(kind) else { return .zero }
    return CGPoint(x: key.frame.midX, y: key.frame.midY)
}

@MainActor
final class FixtureTextTarget: KeyboardTextTarget {
    private(set) var text: String
    let keyboardAppearance: UIKeyboardAppearance?
    var autocapitalizationType: UITextAutocapitalizationType? = .sentences
    var keyboardType: UIKeyboardType? = .default
    var returnKeyType: UIReturnKeyType? = .default

    init(text: String = "", appearance: UIKeyboardAppearance? = nil) {
        self.text = text
        keyboardAppearance = appearance
    }

    var documentContextBeforeInput: String? { text }
    var hasText: Bool { !text.isEmpty }
    func insertText(_ inserted: String) { text += inserted }
    func deleteBackward() { if !text.isEmpty { text.removeLast() } }
}
