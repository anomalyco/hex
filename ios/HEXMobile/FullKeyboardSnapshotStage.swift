import SwiftUI
import UIKit

/// Renders the experimental `FullKeyboardView` under a composer when
/// `--snap-state full-keyboard-<state>[-dark|-dark-field]` is passed on Simulator.
/// `-dark` is a dark app; `-dark-field` is a light app whose field asks for a dark keyboard.
/// The layout follows `--dictation-language` (Automatic, English, without it), or
/// `--keyboard-layout nb|es|de|it|fr|fr-CA|fr-CH|pt|pl|cs|hu|tr|ro|ru|uk|el|sr-Latn|ko|en` pins it; drafts are in the layout's language.
/// `--keyboard-letters qwerty|qwertz|azerty|dvorak|own` arranges its letters (its own without it).
/// `--accent-key ø` (or Cyrillic `е`, `ь`, `ʼ`, Greek `α`, `σ`, `t` on Romanian, or `o` on Hungarian) picks the key the `accents` state holds; `--emoji-query kiss` what `emoji-search` types.
/// Emoji states (`emoji`, `emoji-search`, `emoji-tones`, `emoji-food`, `emoji-strip`) turn
/// Settings › Keyboard › Emoji on with a seeded history; `emoji-probe` prints the panel's
/// footprint and latency (`EMOJI_PROBE` lines) for `simctl launch --console`.
struct FullKeyboardSnapshotStage: View {
    let scene: String
    /// The `sequence` scene's dictation, once it lands in the field.
    @State private var landed: String?

    static var layout: KeyboardLanguage {
        argument(after: "--keyboard-layout").flatMap(KeyboardLanguage.init(rawValue:))
            ?? KeyboardLanguage.automatic(
                dictationLanguage: DictationLanguageFixture.explicit ?? "en-US",
                inputModes: [],
                preferredLanguages: []
            )
    }

    static var accentKey: String {
        argument(after: "--accent-key") ?? "e"
    }

    /// What the `emoji-search` state types (`--emoji-query kiss`).
    static var emojiQuery: String {
        argument(after: "--emoji-query") ?? "heart"
    }

    static func argument(after flag: String) -> String? {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: flag), idx + 1 < args.count else { return nil }
        return args[idx + 1]
    }

    static var requestedScene: String? {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else {
            return nil
        }
        let value = args[idx + 1]
        guard value.hasPrefix("full-keyboard-") else { return nil }
        if let p = args.firstIndex(of: "--hex-placement"), p + 1 < args.count,
           let placement = HexKeyPlacement(rawValue: args[p + 1]) {
            HexKeyPlacement.current = placement
        }
        return value
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
        switch Self.layout {
        case .norwegian:
            return switch state {
            case "inserted": "Kommer ti min for sent, er hjemme 19.30. Begynn uten meg."
            case "numbers": "Kommer ti min for sent, hjemme 19.3"
            case "shift": "Kommer ti min for sent. "
            case "typing": "Kommer ti min for sen"
            case "accents": Self.accentKey == "e" ? "Kaf" : "Bl"
            default: "Kommer ti min for sent, "
            }
        case .spanish:
            return switch state {
            case "inserted": "Llego diez minutos tarde, estaré en casa a las 7:30."
            case "numbers": "Llego diez minutos tarde, en casa a las 7:3"
            case "shift": "Llego diez minutos tarde. "
            case "typing": "Llego diez minutos tard"
            case "accents": "Caf"
            default: "Llego diez minutos tarde, "
            }
        case .german:
            return switch state {
            case "inserted": "Bin zehn Minuten zu spät, um 7:30 zu Hause. Fangt ohne mich an."
            case "numbers": "Bin zehn Minuten zu spät, um 7:3"
            case "shift": "Bin zehn Minuten zu spät. "
            case "typing": "Bin zehn Minuten zu spä"
            case "accents": Self.accentKey == "s" ? "Stra" : "Caf"
            default: "Bin zehn Minuten zu spät, "
            }
        case .italian:
            return switch state {
            case "inserted": "Arrivo con dieci minuti di ritardo, a casa per le 19:30. Iniziate senza di me."
            case "numbers": "Arrivo con dieci minuti di ritardo, a casa per le 19:3"
            case "shift": "Arrivo con dieci minuti di ritardo. "
            case "typing": "Arrivo con dieci minuti di ritsrdo"
            case "accents": Self.accentKey == "e" ? "Perch" : "Cos"
            default: "Arrivo con dieci minuti di ritardo, "
            }
        case .french, .frenchCanadian, .frenchSwiss:
            return switch state {
            case "inserted": "J'arrive avec dix minutes de retard, à la maison vers 19h30. Commencez sans moi."
            case "numbers": "J'arrive avec dix minutes de retard, à la maison vers 19h3"
            case "shift": "J'arrive avec dix minutes de retard. "
            case "typing": "J'arrive avec dix minutes de reta"
            case "accents": "Caf"
            default: "J'arrive avec dix minutes de retard, "
            }
        case .portuguese:
            return switch state {
            case "inserted": "Vou chegar dez minutos atrasado, em casa às 19:30. Comecem sem mim."
            case "numbers": "Vou chegar dez minutos atrasado, em casa às 19:3"
            case "shift": "Vou chegar dez minutos atrasado. "
            case "typing": "Vou chegar dez minutos atras"
            case "accents": "Caf"
            default: "Vou chegar dez minutos atrasado, "
            }
        case .polish:
            return switch state {
            case "inserted": "Spóźnię się dziesięć minut, będę w domu o 19:30. Zacznijcie beze mnie."
            case "numbers": "Spóźnię się dziesięć minut, będę w domu o 19:3"
            case "shift": "Spóźnię się dziesięć minut. "
            case "typing": "Spóźnię się dziesięć min"
            case "accents": Self.accentKey == "e" ? "Prosz" : "Za"
            default: "Spóźnię się dziesięć minut, "
            }
        case .czech:
            return switch state {
            case "inserted": "Přijdu o deset minut později, doma budu v 19:30. Začněte beze mě."
            case "numbers": "Přijdu o deset minut později, doma budu v 19:3"
            case "shift": "Přijdu o deset minut později. "
            case "typing": "Přijdu o deset minut pozd"
            case "accents": Self.accentKey == "r" ? "P" : "Ka"
            default: "Přijdu o deset minut později, "
            }
        case .hungarian:
            return switch state {
            case "inserted": "Köszönöm szépen, találkozunk holnap!"
            case "numbers": "Találkozunk 19:3"
            case "shift": "Köszönöm szépen, találkozunk holnap. "
            case "typing": "Köszönöm szépen, találkozunk holn"
            case "accents": Self.accentKey == "e" ? "Káv" : "Köszön"
            default: "Köszönöm szépen, találkozunk "
            }
        case .turkish:
            return switch state {
            case "inserted": "On dakika gecikeceğim, 19:30'da evde olurum. Bensiz başlayın."
            case "numbers": "On dakika gecikeceğim, 19:3"
            case "shift": "On dakika gecikeceğim. "
            case "typing": "On dakika gecikece"
            case "accents": Self.accentKey == "o" ? "Kah" : "Teşekk"
            default: "On dakika gecikeceğim, "
            }
        case .romanian:
            return switch state {
            case "inserted": "Întârzii zece minute, ajung acasă la 19:30. Începeți fără mine."
            case "numbers": "Întârzii zece minute, ajung acasă la 19:3"
            case "shift": "Întârzii zece minute. "
            case "typing": "Întârzii zece min"
            case "accents": Self.accentKey == "t" ? "Mul" : "Cas"
            default: "Întârzii zece minute, "
            }
        case .russian:
            return switch state {
            case "inserted": "Опаздываю на десять минут, буду дома в 19:30. Начинайте без меня."
            case "numbers": "Опаздываю на десять минут, буду дома в 19:3"
            case "shift": "Опаздываю на десять минут. "
            case "typing": "Опаздываю на десять мину"
            case "accents": Self.accentKey == "ь" ? "Об" : "Ещ"
            default: "Опаздываю на десять минут, "
            }
        case .ukrainian:
            return switch state {
            case "inserted": "Запізнююсь на десять хвилин, буду вдома о 19:30. Починайте без мене."
            case "numbers": "Запізнююсь на десять хвилин, буду вдома о 19:3"
            case "shift": "Запізнююсь на десять хвилин. "
            case "typing": "Запізнююсь на десять хвил"
            case "accents": "Сім"
            default: "Запізнююсь на десять хвилин, "
            }
        case .greek:
            return switch state {
            case "inserted": "Θα αργήσω δέκα λεπτά, θα είμαι σπίτι στις 19:30. Ξεκινήστε χωρίς εμένα."
            case "numbers": "Θα αργήσω δέκα λεπτά, θα είμαι σπίτι στις 19:3"
            case "shift": "Θα αργήσω δέκα λεπτά. "
            case "typing": "Θα αργήσω δέκα λεπ"
            case "accents": Self.accentKey == "σ" ? "Θα αργήσω δέκα λεπτά, θα είμαι σπίτι στι" : "Καλημ"
            default: "Θα αργήσω δέκα λεπτά, "
            }
        case .serbianLatin:
            return switch state {
            case "inserted": "Kasnim deset minuta, biću kod kuće u 19:30. Počnite bez mene."
            case "numbers": "Kasnim deset minuta, biću kod kuće u 19:3"
            case "shift": "Kasnim deset minuta. "
            case "typing": "Kasnim deset minuta, biću kod kuć"
            case "accents": Self.accentKey == "s" ? "Po" : "Laku no"
            default: "Kasnim deset minuta, "
            }
        case .korean:
            return switch state {
            case "inserted": "10분 늦어요. 7시 30분까지 집에 갈게요. 먼저 시작하세요."
            case "numbers": "10분 늦어요. 7시 3"
            case "shift": "10분 늦어요. "
            case "typing": "10분 늦어"
            case "accents": "한"
            default: "10분 늦어요. "
            }
        case .english:
            break
        }
        if state.hasPrefix("emoji") {
            return state == "emoji-strip" ? "Happy birthday " : "Congrats on the launch, pizza tonight? "
        }
        return switch state {
        case "inserted": "Running 10 min late, should be home by 7:30. Start without me."
        case "numbers": "Running 10 min late, home by 7:3"
        case "shift": "Running 10 min late. "
        case "typing": "Running 10 min teh"
        case "accents": "Caf"
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
            Text(landed ?? draft)
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
                .frame(height: KeyboardLayout.height(for: .portrait, numberRow: FullKeyboardSettings().numberRow)
                    + (state == "emoji-search" ? KeyboardMetrics.for(.portrait).stripHeight : 0)
                    // `--short-by 20`: a view shorter than the keyboard asks for, as iOS gives in Spotlight.
                    - (Self.argument(after: "--short-by").flatMap(Double.init).map { CGFloat($0) } ?? 0))
                .background(Color(uiColor: backdrop).ignoresSafeArea(edges: .bottom))
        }
        .background(canvas.ignoresSafeArea())
        .preferredColorScheme(darkApp ? .dark : .light)
        .onReceive(NotificationCenter.default.publisher(for: MotionTrace.landed)) { note in
            landed = (landed ?? draft) + ((note.object as? String) ?? "")
        }
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
        case "language-unavailable":
            // In-keyboard dictation on an iPhone whose Apple Speech can't install Russian.
            let unavailable = SpeechModelUnavailable(language: "ru-RU", reason: .notOnThisIPhone, fallback: "en-US")
            model.configureFixture(
                snapshot: KeyboardSnapshot(
                    state: .failed,
                    heartbeat: now,
                    expiresAt: now + 6,
                    message: UserFacingError.message(for: unavailable, context: .transcription)
                ),
                now: now
            )
        case "no-access", "no-access-hint":
            model.configureFixture(snapshot: .offline, now: now, hasFullAccess: false)
        default:
            model.configureFixture(snapshot: warm, now: now)
        }

        DictationLanguageFixture.apply(to: model)
        let view = FullKeyboardView(model: model, target: context.coordinator.target)
        if state.hasPrefix("emoji") {
            view.emojiOverride = true
            let usage = EmojiFixture.usage()
            view.makeEmojiUsage = { usage }
        }
        let args = ProcessInfo.processInfo.arguments
        if let p = args.firstIndex(of: "--hex-placement"), p + 1 < args.count {
            view.hexPlacementOverride = HexKeyPlacement(rawValue: args[p + 1])
        }
        view.languageOverride = FullKeyboardSnapshotStage.layout
        view.letterLayoutOverride = FullKeyboardSnapshotStage.argument(after: "--keyboard-letters")
            .flatMap(KeyboardLetterLayout.init(rawValue:)) ?? FullKeyboardSnapshotStage.layout.defaultLetterLayout
        view.appearanceOverride = darkKeyboard
        view.orientationOverride = .portrait
        view.drawsBackdrop = true
        view.prepareForField()
        view.warmUpTextCheckerAsync()

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            switch state {
            case "paste-notice":
                view.showAllowPasteHint()
            case "pressed":
                view.debugPress("g")
            case "accents":
                let key = FullKeyboardSnapshotStage.accentKey
                let serbian = FullKeyboardSnapshotStage.layout == .serbianLatin
                let ownLetters: [String: String] = switch FullKeyboardSnapshotStage.layout {
                case .polish: ["e": "ę", "a": "ą", "c": "ć", "l": "ł", "n": "ń", "o": "ó", "s": "ś", "z": "ż"]
                case .romanian: ["a": "ă", "i": "î", "s": "ș", "t": "ț"]
                case .czech: ["e": "ě", "r": "ř", "u": "ů", "a": "á", "c": "č", "s": "š", "z": "ž", "y": "ý", "n": "ň"]
                case .hungarian: ["a": "á", "e": "é", "i": "í", "o": "ő", "u": "ű"]
                case .turkish: ["a": "â", "i": "î", "o": "ô", "u": "û", "s": "ś", "c": "ć"]
                case .greek: ["ε": "έ", "α": "ά", "ο": "ό", "η": "ή", "ι": "ί", "υ": "ύ", "ω": "ώ", "σ": "ς", "κ": "ϗ"]
                default: [:]
                }
                let pick = ownLetters[key] ?? [
                    "e": "é", "s": serbian ? "š" : "ß", "ø": "ö", "æ": "ä", "a": "á", "o": "ó", "n": "ñ", "u": "ú",
                    "i": "ì", "е": "ё", "ь": "ъ", "c": "ć", "ʼ": "’",
                ][key]
                view.debugHoldAccent(key, selecting: pick ?? key)
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
            // The bar's other states, set on the strip directly: a revert, Paste, Paste dictation,
            // Code from Messages (with and without a copied code) and the two-thumb selection.
            case "strip-revert":
                view.stripView.update(suggestions: [
                    KeyboardSuggestion(displayText: "teh", replacement: "teh", kind: .revert),
                    KeyboardSuggestion(displayText: "then", replacement: "then", kind: .secondary),
                    KeyboardSuggestion(displayText: "tech", replacement: "tech", kind: .secondary),
                ], showsUndo: false, statusText: nil)
            case "strip-two":
                view.stripView.update(suggestions: [
                    KeyboardSuggestion(displayText: "\u{201C}lol\u{201D}", replacement: "lol", kind: .keepLiteral),
                    KeyboardSuggestion(displayText: "lot", replacement: "lot", kind: .secondary),
                ], showsUndo: false, statusText: nil)
            case "strip-paste":
                view.stripView.update(suggestions: [], showsUndo: false, statusText: nil, clipboard: .paste)
            case "strip-paste-dictation":
                view.stripView.update(suggestions: [], showsUndo: false, statusText: nil, clipboard: .dictation)
            case "strip-code":
                view.stripView.update(suggestions: [], showsUndo: false, statusText: nil, clipboard: .code(canPaste: true))
            case "strip-selection":
                view.stripView.isHidden = true
                view.selectionStrip.update(TrackpadSelectionSession.Presentation(phase: .actions, before: "Running 10 ", selected: "min late", after: ", home by"), canUseClipboard: true)
            case "emoji":
                view.debugOpenEmoji()
            case "emoji-search":
                view.debugOpenEmoji(search: FullKeyboardSnapshotStage.emojiQuery)
            case "emoji-tones":
                view.debugOpenEmoji()
                view.emojiPanel?.debugShowTones(for: "👍", selecting: 3)
            case "emoji-food":
                view.debugOpenEmoji()
                view.emojiPanel?.debugJump(toSection: 3)
            case "emoji-probe":
                EmojiFixture.probe(view)
            case "no-access-hint":
                // The Hex key tapped without Full Access.
                view.showFullAccessHint()
            case "style":
                view.debugShowStyleMenu()
            case "languages":
                view.debugShowStyleMenu(page: .languages)
            case "languages-remove":
                view.debugOfferRemove(model.languageChoices[1])
            case "more-languages":
                view.debugShowStyleMenu(page: .moreLanguages)
            case "translation":
                view.debugShowStyleMenu(page: .translation)
            case "morph":
                // Loops idle → recording → idle so the strip morph can be screen-recorded.
                func cycle(_ start: TimeInterval) {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                        model.configureFixture(
                            snapshot: KeyboardSnapshot(
                                state: .recording,
                                heartbeat: start,
                                expiresAt: start + 15 * 60,
                                recordingStartedAt: start - 1,
                                audioLevel: 0.7,
                                loudnessDB: -30,
                                jobID: "snap-job"
                            ),
                            now: start
                        )
                        DispatchQueue.main.asyncAfter(deadline: .now() + 2.4) {
                            model.configureFixture(snapshot: warm, now: start)
                            cycle(start)
                        }
                    }
                }
                cycle(now)
            case "strip-sequence":
                // The bar's own transitions, set on the strip directly: words
                // appearing, changing and leaving, an autocorrection's bubble, an emoji, a revert,
                // Undo dictation in and out, and fast typing retargeting a word mid-swap.
                let slow = MotionDebug.slow
                if slow > 1 { view.window?.layer.speed = Float(1 / slow) }
                if MotionDebug.tracing { MotionTrace.shared.start(view) }
                func word(_ text: String, _ kind: KeyboardSuggestion.Kind) -> KeyboardSuggestion {
                    KeyboardSuggestion(displayText: text, replacement: text, kind: kind)
                }
                func at(_ seconds: Double, _ mark: String, _ suggestions: [KeyboardSuggestion], undo: Bool = false) {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 3.5 + seconds * slow) {
                        MotionTrace.shared.mark(mark)
                        view.stripView.update(suggestions: suggestions, showsUndo: undo, statusText: nil)
                    }
                }
                view.stripView.update(suggestions: [], showsUndo: false, statusText: nil)
                at(0.3, "appear", [word("\u{201C}Th\u{201D}", .keepLiteral), word("The", .primary(autoReplace: false)), word("This", .secondary)])
                at(1.1, "change", [word("\u{201C}The\u{201D}", .keepLiteral), word("They", .primary(autoReplace: false)), word("There", .secondary)])
                at(1.9, "bubble", [word("\u{201C}Thw\u{201D}", .keepLiteral), word("The", .primary(autoReplace: true)), word("Thaw", .secondary)])
                at(2.7, "emoji", [word("🙏", .emoji)])
                at(3.5, "revert", [word("teh", .revert), word("then", .secondary), word("tech", .secondary)])
                at(4.3, "undo", [], undo: true)
                at(5.5, "undo-leaves", [word("I", .nextWord), word("The", .nextWord), word("I'm", .nextWord)])
                at(6.3, "clear", [])
                at(7.0, "type-r", [word("Ru", .primary(autoReplace: false))])
                at(7.08, "type-u", [word("Run", .primary(autoReplace: false))])
                at(7.16, "type-n", [word("Running", .primary(autoReplace: false))])
                DispatchQueue.main.asyncAfter(deadline: .now() + 3.5 + 8.0 * slow) { MotionTrace.shared.finish() }
            case "sequence", "cancel-sequence":
                // Rest, then a recording on the demo voice (speech, pause, speech). `sequence`
                // ends with ✓: transcribing, the text lands, the suggestions come back;
                // `cancel-sequence` ends with ✕. `--motion-slow 4` plays all of it at a quarter.
                let slow = MotionDebug.slow
                if slow > 1 { view.window?.layer.speed = Float(1 / slow) }
                if MotionDebug.tracing { MotionTrace.shared.start(view) }
                let cancels = state == "cancel-sequence"
                func at(_ seconds: Double, _ step: @escaping () -> Void) {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 3.5 + seconds * slow, execute: step)
                }
                // A finger on the Hex key for a moment, then the tap lifts and recording starts.
                at(-0.15) { view.debugPressHexKey(true) }
                at(0) {
                    view.debugPressHexKey(false)
                    MotionTrace.shared.mark("record")
                    model.configureFixture(
                        snapshot: KeyboardSnapshot(
                            state: .recording,
                            heartbeat: now,
                            expiresAt: now + 15 * 60,
                            recordingStartedAt: now,
                            audioLevel: 0,
                            loudnessDB: -120,
                            jobID: "snap-job"
                        ),
                        now: now
                    )
                }
                if cancels {
                    at(2.6) {
                        MotionTrace.shared.mark("cancel")
                        model.configureFixture(snapshot: warm, now: now)
                    }
                    at(4.0) { MotionTrace.shared.finish() }
                } else {
                    at(3.6) {
                        MotionTrace.shared.mark("done")
                        model.configureFixture(
                            snapshot: KeyboardSnapshot(state: .transcribing, heartbeat: now, expiresAt: now + 15 * 60, jobID: "snap-job"),
                            now: now
                        )
                    }
                    at(5.0) {
                        MotionTrace.shared.mark("landed")
                        let text = "should be home by 7:30."
                        NotificationCenter.default.post(name: MotionTrace.landed, object: text)
                        view.didInsertDictation(text)
                        model.configureFixture(snapshot: warm, now: now)
                    }
                    at(6.6) { MotionTrace.shared.finish() }
                }
            default:
                break
            }
        }
        return view
    }

    func updateUIView(_ uiView: FullKeyboardView, context: Context) {}
}

/// A seeded emoji history for the scenes, and the probe that measures the panel.
@MainActor
enum EmojiFixture {
    static func usage() -> EmojiUsageStore {
        let defaults = UserDefaults(suiteName: "anomaly.hex.fixture.emoji")
        defaults?.removePersistentDomain(forName: "anomaly.hex.fixture.emoji")
        let store = EmojiUsageStore(defaults: defaults)
        for (emoji, times) in [("😂", 6), ("❤️", 5), ("👍", 4), ("🙏", 3), ("🔥", 3), ("😭", 2), ("🥹", 2), ("☕", 1)] {
            for _ in 0..<times { store.record(emoji) }
        }
        store.setTone(3, for: "👍")
        store.setTone(3, for: "🙏")
        return store
    }

    private static func footprint() -> Double {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
    }

    private static func log(_ message: String) {
        print("EMOJI_PROBE \(message) footprint=\(String(format: "%.2f", footprint()))MB")
    }

    /// Opens the panel, pages through every section, searches, closes, and repeats the open
    /// ten times, printing footprints and the open latencies.
    static func probe(_ view: FullKeyboardView) {
        log("keyboard")
        var steps: [(TimeInterval, () -> Void)] = []
        var latencies: [Double] = []
        steps.append((1.0, { view.debugOpenEmoji(); latencies.append(view.lastEmojiPanelLatency ?? -1) }))
        steps.append((1.0, { log("panel open, first time, latency=\(String(format: "%.1f", latencies[0] * 1000))ms") }))
        // A category tapped every 8 frames, each for the first time, then each again: the
        // longest frame after each tap (60 fps is 16.7 ms).
        steps.append((0.3, {
            let order = [3, 7, 1, 5, 8, 2, 6, 4, 3, 7, 1, 5, 8, 2, 6, 4, 0]
            var frame = 0
            var lastTick: CFTimeInterval = 0
            var worst: [Double] = Array(repeating: 0, count: order.count)
            FrameProbe.run(step: {
                let now = CACurrentMediaTime()
                let tap = frame / 8
                if lastTick > 0, tap > 0, frame % 8 != 0 || tap <= order.count {
                    let owner = frame % 8 == 0 ? tap - 1 : tap
                    if owner < order.count { worst[owner] = max(worst[owner], now - lastTick) }
                }
                lastTick = now
                if frame % 8 == 0, tap < order.count { view.emojiPanel?.debugJump(toSection: order[tap]) }
                frame += 1
                return tap < order.count
            }) { _, _ in
                func ms(_ values: ArraySlice<Double>) -> String { values.map { String(format: "%.0f", $0 * 1000) }.joined(separator: " ") }
                log("category taps, longest frame after each (ms): first visits \(ms(worst[0..<8])) | again \(ms(worst[8..<16]))")
            }
        }))
        steps.append((3.0, {}))
        for section in 1...8 {
            steps.append((0.3, { view.emojiPanel?.debugJump(toSection: section) }))
        }
        steps.append((0.5, { log("panel open, every section drawn") }))
        // Every emoji: the grid scrolled from the first column to the last, a screen at a time.
        steps.append((0.3, { view.emojiPanel?.debugJump(toSection: 0) }))
        for page in 0..<60 {
            steps.append((0.05, { view.emojiPanel?.debugScroll(toPage: page) }))
        }
        steps.append((0.5, {
            guard let glyphs = view.emojiPanel?.glyphs else { return }
            let average = glyphs.renders > 0 ? glyphs.renderTime / Double(glyphs.renders) * 1000 : 0
            log("panel open, every emoji drawn, renders=\(glyphs.renders) fonts=\(glyphs.fontsMade) slowest font=\(String(format: "%.2f", glyphs.slowestFont * 1000))ms private=\(glyphs.hasPrivateFont) render avg=\(String(format: "%.2f", average))ms max=\(String(format: "%.2f", glyphs.slowestRender * 1000))ms cached=\(glyphs.cachedCount) \(glyphs.cachedBytes / 1024)KB")
        }))
        // A fast fling's pace, 40 pt a frame from the start to the end: each frame's work.
        steps.append((0.3, { view.emojiPanel?.debugJump(toSection: 1) }))
        steps.append((0.5, {
            FrameProbe.run(step: { view.emojiPanel?.debugScroll(by: 40) ?? false }) { work, gaps in
                let sorted = work.sorted()
                func ms(_ value: Double) -> String { String(format: "%.2f", value * 1000) }
                log("scrolled every emoji at 40 pt a frame, frames=\(sorted.count) work p50=\(ms(sorted[sorted.count / 2]))ms p95=\(ms(sorted[sorted.count * 95 / 100]))ms max=\(ms(sorted.last ?? 0))ms frames over 16.7ms=\(sorted.filter { $0 > 1.0 / 60 }.count) late frames=\(gaps)")
            }
        }))
        steps.append((6.0, {}))
        steps.append((0.2, {
            view.beginEmojiSearch()
            for character in "pizza" { view.routedTarget.insertText(String(character)) }
            view.refreshEmojiSearch()
        }))
        steps.append((0.5, { log("searching") }))
        steps.append((0.2, { view.closeEmojiPanel() }))
        steps.append((1.0, { log("panel closed") }))
        for _ in 0..<10 {
            steps.append((0.3, { view.openEmojiPanel(); latencies.append(view.lastEmojiPanelLatency ?? -1) }))
            steps.append((0.3, { view.closeEmojiPanel() }))
        }
        steps.append((1.0, {
            let warm = latencies.dropFirst().map { $0 * 1000 }.sorted()
            log("panel closed after 11 opens, warm open latency median=\(String(format: "%.1f", warm[warm.count / 2]))ms max=\(String(format: "%.1f", warm.last ?? 0))ms")
            let before = footprint()
            let start = CACurrentMediaTime()
            let library = EmojiLibrary.load(language: "en", bundle: Bundle(for: FullKeyboardView.self))
            let semantic = library.flatMap { EmojiSemanticIndex(language: "en", vocabulary: $0.vocabulary) }
            let built = CACurrentMediaTime() - start
            log("semantic index built=\(semantic != nil) in \(String(format: "%.0f", built * 1000))ms, +\(String(format: "%.2f", footprint() - before))MB")
            _ = semantic?.neighbors(of: "congrats")
        }))
        func run(_ index: Int) {
            guard index < steps.count else {
                log("done")
                return
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + steps[index].0) {
                steps[index].1()
                run(index + 1)
            }
        }
        run(0)
    }
}

/// Probes: calls `step` once a frame until it returns false, timing each call, then
/// reports the times and how many frames came late (over 1.5 frames after the last).
@MainActor
final class FrameProbe: NSObject {
    private let step: () -> Bool
    private let done: ([Double], Int) -> Void
    private var link: CADisplayLink?
    private var work: [Double] = []
    private var last: CFTimeInterval = 0
    private var late = 0
    private static var running: [FrameProbe] = []

    private init(step: @escaping () -> Bool, done: @escaping ([Double], Int) -> Void) {
        self.step = step
        self.done = done
    }

    static func run(step: @escaping () -> Bool, done: @escaping ([Double], Int) -> Void) {
        let probe = FrameProbe(step: step, done: done)
        running.append(probe)
        let link = CADisplayLink(target: probe, selector: #selector(tick(_:)))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 60, preferred: 60)
        link.add(to: .main, forMode: .common)
        probe.link = link
    }

    @objc private func tick(_ link: CADisplayLink) {
        if last > 0, link.timestamp - last > 1.5 / 60 { late += 1 }
        last = link.timestamp
        let started = CACurrentMediaTime()
        let more = step()
        work.append(CACurrentMediaTime() - started)
        guard !more else { return }
        link.invalidate()
        done(work, late)
        Self.running.removeAll { $0 === self }
    }
}

@MainActor
private func center(of kind: KeyKind, in view: FullKeyboardView) -> CGPoint {
    guard let key = view.engine.layout.key(kind) else { return .zero }
    return CGPoint(x: key.frame.midX, y: key.frame.midY)
}

@MainActor
class FixtureTextTarget: KeyboardTextTarget {
    private(set) var text: String
    private(set) var cursorIndex: Int
    let keyboardAppearance: UIKeyboardAppearance?
    var autocapitalizationType: UITextAutocapitalizationType? = .sentences
    var autocorrectionType: UITextAutocorrectionType? = .default
    var keyboardType: UIKeyboardType? = .default
    var returnKeyType: UIReturnKeyType? = .default
    var enablesReturnKeyAutomatically: Bool? = false
    var smartQuotesType: UITextSmartQuotesType?
    var smartDashesType: UITextSmartDashesType?
    var isSecureTextEntry: Bool?
    var textContentType: UITextContentType?
    var selectedText: String?
    var documentIdentifier: UUID?
    /// Simulates a host whose window reports no text after the cursor.
    var hidesAfterText = false

    init(text: String = "", appearance: UIKeyboardAppearance? = nil) {
        self.text = text
        self.cursorIndex = text.count
        keyboardAppearance = appearance
    }

    var documentContextBeforeInput: String? {
        let clamped = max(0, min(text.count, cursorIndex))
        let idx = text.index(text.startIndex, offsetBy: clamped)
        return String(text[..<idx])
    }

    var documentContextAfterInput: String? {
        hidesAfterText ? nil : String(text.dropFirst(max(0, min(text.count, cursorIndex))))
    }

    var hasText: Bool { !text.isEmpty }

    func insertText(_ inserted: String) {
        let clamped = max(0, min(text.count, cursorIndex))
        let idx = text.index(text.startIndex, offsetBy: clamped)
        text.insert(contentsOf: inserted, at: idx)
        cursorIndex = clamped + inserted.count
    }

    func deleteBackward() {
        let clamped = max(0, min(text.count, cursorIndex))
        guard clamped > 0 else { return }
        let removeIdx = text.index(text.startIndex, offsetBy: clamped - 1)
        text.remove(at: removeIdx)
        cursorIndex = clamped - 1
    }

    func adjustTextPosition(byCharacterOffset offset: Int) {
        cursorIndex = max(0, min(text.count, cursorIndex + offset))
    }
}
/// Per-frame samples of the recording strip (`--motion-trace`), written as
/// NDJSON to Documents/motion-trace.ndjson; `t` is scene time (real time over `--motion-slow`).
@MainActor
final class MotionTrace: NSObject {
    static let shared = MotionTrace()
    static let landed = Notification.Name("MotionTrace.landed")
    private weak var view: FullKeyboardView?
    private var link: CADisplayLink?
    private var start: CFTimeInterval = 0
    private var lines: [String] = []

    func start(_ view: FullKeyboardView) {
        self.view = view
        start = CACurrentMediaTime()
        let link = CADisplayLink(target: self, selector: #selector(tick))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    private var now: Double { (CACurrentMediaTime() - start) / MotionDebug.slow }

    func mark(_ event: String) {
        guard link != nil else { return }
        lines.append("{\"t\":\(now),\"event\":\"\(event)\"}")
    }

    @objc private func tick() {
        guard let view else { return }
        var sample = view.debugMotionSample()
        sample["t"] = now
        if let data = try? JSONSerialization.data(withJSONObject: sample, options: [.sortedKeys]),
           let line = String(data: data, encoding: .utf8) {
            lines.append(line)
        }
        if lines.count % 30 == 0 { flush() }
    }

    private func flush() {
        let url = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("motion-trace.ndjson")
        try? (lines.joined(separator: "\n") + "\n").write(to: url, atomically: true, encoding: .utf8)
    }

    func finish() {
        guard let link else { return }
        link.invalidate()
        self.link = nil
        flush()
    }
}
