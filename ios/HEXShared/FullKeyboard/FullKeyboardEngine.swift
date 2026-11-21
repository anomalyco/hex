import QuartzCore
import UIKit

/// The text field the keys type into. `UITextDocumentProxy` in the keyboard; a fake in tests.
@MainActor
protocol KeyboardTextTarget: AnyObject {
    var documentContextBeforeInput: String? { get }
    var hasText: Bool { get }
    var autocapitalizationType: UITextAutocapitalizationType? { get }
    var autocorrectionType: UITextAutocorrectionType? { get }
    var keyboardType: UIKeyboardType? { get }
    var returnKeyType: UIReturnKeyType? { get }
    var enablesReturnKeyAutomatically: Bool? { get }
    var keyboardAppearance: UIKeyboardAppearance? { get }
    var textContentType: UITextContentType? { get }
    var isSecureTextEntry: Bool? { get }
    func insertText(_ text: String)
    func deleteBackward()
    func adjustTextPosition(byCharacterOffset offset: Int)
}

extension KeyboardTextTarget {
    var autocorrectionType: UITextAutocorrectionType? { .default }
    var enablesReturnKeyAutomatically: Bool? { false }
    var textContentType: UITextContentType? { nil }
    var isSecureTextEntry: Bool? { nil }
    func adjustTextPosition(byCharacterOffset offset: Int) {}
}

@MainActor
final class DocumentProxyTextTarget: KeyboardTextTarget {
    private let proxy: () -> UITextDocumentProxy?

    init(_ proxy: @escaping () -> UITextDocumentProxy?) {
        self.proxy = proxy
    }

    var documentContextBeforeInput: String? { proxy()?.documentContextBeforeInput }
    var hasText: Bool { proxy()?.hasText ?? false }
    var autocapitalizationType: UITextAutocapitalizationType? { proxy()?.autocapitalizationType }
    var autocorrectionType: UITextAutocorrectionType? { proxy()?.autocorrectionType }
    var keyboardType: UIKeyboardType? { proxy()?.keyboardType }
    var returnKeyType: UIReturnKeyType? { proxy()?.returnKeyType }
    var enablesReturnKeyAutomatically: Bool? { proxy()?.enablesReturnKeyAutomatically }
    var keyboardAppearance: UIKeyboardAppearance? { proxy()?.keyboardAppearance }
    var textContentType: UITextContentType? { proxy()?.textContentType ?? nil }
    var isSecureTextEntry: Bool? { proxy()?.isSecureTextEntry }
    func insertText(_ text: String) { proxy()?.insertText(text) }
    func deleteBackward() { proxy()?.deleteBackward() }
    func adjustTextPosition(byCharacterOffset offset: Int) { proxy()?.adjustTextPosition(byCharacterOffset: offset) }
}

/// Everything the keys do, apart from drawing: multitouch with rollover, shift, pages,
/// long-press accents, spacebar cursor trackpad, cautious autocorrect, double-space period,
/// auto-capitalization and accelerating delete. Touches are identified by any stable integer
/// so tests can drive it without `UITouch`.
@MainActor
final class FullKeyboardEngine {
    struct ActiveTouch: Equatable {
        var key: KeyboardKey
        var startPoint: CGPoint
        var point: CGPoint
        /// A character that already went in because another key went down after it.
        var committed: Bool
        /// The page a 123/ABC press came from; releasing over a character returns there.
        var returnLayer: KeyboardLayer?
        /// A 123, #+= or shift press slid onto this character key: it shows the key's pop-up,
        /// and letting go types it (`slideTarget`), as on Apple's keyboard.
        var slideKey: KeyboardKey?
        /// True once such a press slid onto a character key, so letting go back on the page
        /// key is a slide taken back rather than a tap.
        var hasSlid = false
        /// True once a spacebar press crosses `trackpadActivationDistance`.
        var isTrackpad = false
        /// True once a spacebar press slides before its hold (`holdSpace`) while it switches
        /// language: letting go far enough along switches instead of typing a space.
        var isLanguageSwipe = false
        var trackpadAnchorX: CGFloat = 0
        /// Characters the trackpad has moved the cursor, for Record typing logs.
        var trackpadOffset = 0
        /// True when the likelier next letter claimed this touch from the key under it; a
        /// small move then keeps it rather than sliding back (`letterReach`).
        var claimed = false
        /// Where the finger went down, before a strip touch moved onto row 1 (Adaptive touch).
        var downPoint: CGPoint = .zero
    }

    /// Apple's callout replaces the key's pop-up about 382 ms after it appeared (iOS 27.2, 60 fps
    /// recordings); Hex's took 420 at 0.4 s, a frame later to show.
    static let accentHoldDelay: TimeInterval = 0.36
    /// How far below the held key's middle a finger lets go to close the callout untyped.
    static let accentCancelDrop: CGFloat = 57.5
    static let accentSlop: CGFloat = 10
    static let trackpadActivationDistance: CGFloat = 10
    static let trackpadPointsPerCharacter: CGFloat = 7.5
    /// Holding the space bar this long without sliding starts the trackpad, as Apple's does;
    /// letting go then types no space. With two or more languages a slide moves the cursor
    /// only after it; a slide before it is a language swipe.
    /// Apple's space bar turns into the trackpad about 363 ms after touch-down (iOS 27.2, 60 fps).
    static let trackpadHoldDelay: TimeInterval = 0.35
    /// A finger on the strip's lower half this soon after a key went down or came up (or
    /// while one is down) types the row-1 key below it (`takesStripTouch(at:)`).
    static let stripTouchGuard: TimeInterval = 0.25
    /// How far along the space bar a language swipe must go; a shorter one types a space.
    static let languageSwipeDistance: CGFloat = 24

    private(set) var shift = ShiftStateMachine()
    private(set) var layerState = KeyboardLayerState()
    private(set) var field: KeyboardFieldKind = .text
    private(set) var touches: [Int: ActiveTouch] = [:]
    /// Touch ids in the order they went down.
    private var touchOrder: [Int] = []
    private var lastSpaceAt: TimeInterval?
    /// The text before the cursor just after a suggestion typed its word and a space. A space
    /// typed next, while the text is still that, is swallowed, as on Apple's keyboard.
    private var suggestionSpaceBefore: String?
    /// When a key last went down or came up, for `takesStripTouch(at:)`.
    private var lastKeyTouchAt: TimeInterval?

    /// Where each letter of the word being typed was tapped (nil for one that came another
    /// way, such as an accent), for Spatial autocorrect.
    private var tokenTaps: [CGPoint?] = []
    /// Spatial autocorrect: where you tend to tap each letter, from its centre (Learn how I type).
    var tapOffsets: [Character: CGVector] = [:] {
        didSet { if tapOffsets != oldValue { syncDecoderKeys() } }
    }

    /// The layout language's word list, which lets the likelier of two neighbouring letters
    /// claim a few points of the other's touch target, as Apple's keyboard does ("th" then a
    /// tap on the e/d border types e). Used only while autocorrect is on in a text field
    /// that allows it; nil keeps every target fixed.
    var letterOdds: TypingDecoderModel? {
        didSet {
            if letterOdds !== oldValue {
                continuationCache = nil
                wordStartMass = nil
                autocorrect.contextModel = letterOdds
            }
        }
    }
    /// How far past its own target a likelier letter may reach, in points.
    var letterReach: CGFloat = 5
    /// Nats of odds (ln of how many times likelier) a letter needs for each point it claims.
    var letterOddsPerPoint = 0.8
    private var continuationCache: (prefix: [UInt8], mass: [Double]?)?
    /// The first letters' frequencies, the costliest to add up (every word), kept apart.
    private var wordStartMass: [Double]??

    private(set) var activeAccentMenu: AccentMenu?
    private var activeAccentTouchID: Int?
    private(set) var suggestions: [KeyboardSuggestion] = []

    let target: KeyboardTextTarget
    let autocorrect: AutocorrectEngine
    /// Learn how I type (Experiments); nil while off.
    var journal: TypingJournal? {
        didSet { syncLearning() }
    }

    private var learning: TypingJournal? {
        guard let journal, TypingJournal.allows(field: field, target: target) else { return nil }
        return journal
    }
    /// Record typing logs (Experiments); nil while it is off, in fields it skips, and in the
    /// app's fixtures. Every call through it is skipped (arguments too) while nil.
    var typingLog: TypingLogger?
    /// Adaptive touch (Experiments): personal key centres for Spatial autocorrect, in place of
    /// `tapOffsets`, learned from taps with evidence of what you meant. Nil while off.
    var adaptiveTouch: AdaptiveTouch? {
        didSet {
            guard adaptiveTouch !== oldValue else { return }
            adaptiveTouch?.onChange = { [weak self] in self?.syncDecoderKeys() }
            adaptiveTouch?.clock = { [weak self] in self?.clock() ?? CACurrentMediaTime() }
            adaptiveTouch?.forget()
            syncAdaptiveField()
            syncDecoderKeys()
        }
    }

    /// Adaptive touch where this field lets it learn (plain text that allows autocorrect).
    private var adaptiveLearning: AdaptiveTouch?

    private func syncAdaptiveField() {
        adaptiveLearning = adaptiveTouch.flatMap { AdaptiveTouch.allows(field: field, target: target) ? $0 : nil }
    }
    var width: CGFloat = 375 { didSet { if width != oldValue { rebuildLayout() } } }
    var orientation: KeyboardOrientation = .portrait { didSet { if orientation != oldValue { rebuildLayout() } } }
    var showsGlobe = false { didSet { if showsGlobe != oldValue { rebuildLayout() } } }
    /// Settings › Keyboard › Emoji, in fields that take emoji.
    var showsEmoji = false { didSet { if showsEmoji != oldValue { rebuildLayout() } } }
    var horizontalInset: CGFloat = 0 { didSet { if horizontalInset != oldValue { rebuildLayout() } } }
    /// An iPhone, or the iPhone Duo folded or unfolded (`KeyboardPosture`).
    var posture: KeyboardPosture = .phone { didSet { if posture != oldValue { rebuildLayout() } } }
    /// Which iPhone key plane to draw (`KeyboardPhoneSize`), from the screen's size.
    var phoneSize: KeyboardPhoneSize = .regular { didSet { if phoneSize != oldValue { rebuildLayout() } } }
    /// How far iOS set the keyboard in from each side of the screen (the Duo).
    var windowInset: CGFloat = 0 { didSet { if windowInset != oldValue { rebuildLayout() } } }
    /// Settings › Keyboard › Hex key: the strip's trailing (Right) or leading (Left) end.
    var hexPlacement: HexKeyPlacement = .stripTrailing {
        didSet { if hexPlacement != oldValue { rebuildLayout(cause: "hex_placement") } }
    }
    /// The letters, accents and spelling dictionary (Settings › Keyboard › Languages).
    var language: KeyboardLanguage = .english {
        didSet {
            guard language != oldValue else { return }
            cancelAllTouches()
            autocorrect.setLanguage(language)
            rebuildLayout(cause: "language")
            refreshSuggestions()
        }
    }
    /// The letters and long presses on the keys when not `language`'s (`KeyboardLanguageList.layout`).
    var layoutLanguage: KeyboardLanguage? {
        didSet { if layoutLanguage != oldValue { cancelAllTouches(); rebuildLayout(cause: "language") } }
    }
    /// How the keys' language arranges its letters (Settings › Keyboard › Languages › Layout);
    /// nil, or one it doesn't offer, is its own.
    var letterLayout: KeyboardLetterLayout? {
        didSet { if letterLayout != oldValue { cancelAllTouches(); rebuildLayout(cause: "letters") } }
    }
    /// Bilingual autocorrect's second language on these keys (`KeyboardLanguageList.partner`),
    /// or nil.
    var partnerLanguage: KeyboardLanguage? {
        get { autocorrect.partner?.language }
        set {
            guard newValue != autocorrect.partner?.language else { return }
            autocorrect.setPartner(newValue)
            refreshSuggestions()
        }
    }
    /// Settings › Keyboard › Languages has two or more: a quick swipe along the space bar
    /// switches language (`onLanguageSwipe`) and the cursor slides once the space bar is held
    /// (`trackpadHoldDelay`), as on SwiftKey. With one, any slide moves the cursor at once.
    var switchesLanguage = false
    private(set) var layout = KeyboardLayout(width: 375)

    var clock: () -> TimeInterval = { CACurrentMediaTime() }
    /// Called whenever something visible changed (page, shift, pressed keys, suggestions, accents).
    var onChange: (() -> Void)?
    /// Called after the keys typed or deleted anything.
    var onEdit: (() -> Void)?
    /// Called when an accent cell or spacebar trackpad step ticks, for selection haptics.
    var onSelectionStep: (() -> Void)?
    /// Called just before the return key types its line break, so a send can be noticed.
    var onReturn: (() -> Void)?
    /// Called when the emoji key is let go.
    var onEmojiKey: (() -> Void)?
    /// Called when a swipe along the space bar switches language: 1 for the next (a swipe
    /// to the left), -1 for the one before.
    var onLanguageSwipe: ((Int) -> Void)?

    init(target: KeyboardTextTarget, autocorrect: AutocorrectEngine? = nil) {
        self.target = target
        self.autocorrect = autocorrect ?? AutocorrectEngine()
        rebuildLayout()
    }

    var isUppercase: Bool { layerState.layer == .letters && shift.isUppercase }

    var isTrackpadActive: Bool {
        touches.values.contains(where: \.isTrackpad)
    }

    /// Settings › Key pop-ups. Off: pressed keys only darken, as with iOS's Character Preview off.
    var showsKeyPopups = true

    /// Settings › Keyboard › Auto-capitalization. Off, shift never turns itself on, whatever
    /// the field's `autocapitalizationType` asks, and autocorrect and swipe typing add no
    /// capitals (`AutocorrectEngine.addsCapitals`). Shift and caps lock still work.
    var autoCapitalizes = true {
        didSet {
            guard autoCapitalizes != oldValue else { return }
            autocorrect.addsCapitals = autoCapitalizes
            refreshAutoCapitalization()
            refreshSuggestions()
        }
    }

    /// What the field asks for, or no automatic capitals with Auto-capitalization off.
    var autoCapitalizationMode: AutoCapitalization.Mode {
        autoCapitalizes ? AutoCapitalization.Mode(target.autocapitalizationType) : .none
    }

    /// The key whose balloon shows: the newest character still held, or the one a 123, #+= or
    /// shift press slid onto (hidden while an accent menu is up).
    var previewKey: KeyboardKey? {
        guard activeAccentMenu == nil else { return nil }
        for id in touchOrder.reversed() {
            guard let touch = touches[id] else { continue }
            if let slid = touch.slideKey { return slid }
            if touch.key.character != nil, !touch.committed {
                return touch.key
            }
        }
        return nil
    }

    var pressedKinds: Set<KeyKind> {
        Set(touches.values.flatMap { touch in [touch.key.kind] + (touch.slideKey.map { [$0.kind] } ?? []) })
    }

    func displayLabel(for key: KeyboardKey) -> String {
        guard let character = key.character else { return key.label }
        return isUppercase && layerState.layer == .letters ? character.uppercased() : character
    }

    /// Whether the letter keys read as capitals: as `isUppercase`, except while a press that
    /// turns shift on is still down (`ShiftStateMachine.pressShowsOff`).
    var keycapsUppercase: Bool { isUppercase && !shift.pressShowsOff }

    /// A key's label as its keycap shows it (`keycapsUppercase`); pop-ups use `displayLabel`.
    func keycapLabel(for key: KeyboardKey) -> String {
        guard let character = key.character else { return key.label }
        return keycapsUppercase ? character.uppercased() : character
    }

    // MARK: - Field

    /// Call when the keyboard appears or the field changes.
    func configureForField() {
        switch KeyboardFieldRouting.route(for: target.keyboardType) {
        case .keys(let kind, let initialLayer):
            field = kind
            layerState.reset(to: initialLayer)
        case .handOff:
            field = .text
            layerState.reset(to: .numbers)
        }
        shift.reset()
        lastSpaceAt = nil
        suggestionSpaceBefore = nil
        tokenTaps.removeAll()
        swipedWord = nil
        finishedWord = nil
        autocorrect.clearPendingUndo()
        adaptiveTouch?.beginSession()
        syncAdaptiveField()
        cancelAllTouches()
        rebuildLayout(cause: "field")
        refreshAutoCapitalization()
        refreshSuggestions()
    }

    /// Call after the text or the cursor changed outside these keys.
    func textDidChange() {
        lastSpaceAt = nil
        // Hosts that report the keys' own edits leave the text as the suggestion left it.
        if target.documentContextBeforeInput != suggestionSpaceBefore {
            suggestionSpaceBefore = nil
        }
        tokenTaps.removeAll()
        swipedWord = nil
        finishedWord = nil
        autocorrect.clearPendingUndo()
        syncAdaptiveField()
        adaptiveLearning?.hostChanged(before: target.documentContextBeforeInput)
        refreshAutoCapitalization()
        refreshSuggestions()
    }

    func refreshAutoCapitalization() {
        let before = shift
        shift.refreshAutoCapitalization(
            AutoCapitalization.shouldCapitalize(
                mode: autoCapitalizationMode,
                before: target.documentContextBeforeInput,
                hasText: target.hasText
            )
        )
        if shift != before {
            logState("auto")
            onChange?()
        }
    }

    func refreshSuggestions() {
        syncLearning()
        let next: [KeyboardSuggestion]
        if let swipeSuggestions {
            next = swipeSuggestions
        } else {
            autocorrect.decoderTaps = currentTokenTaps
            next = autocorrect.suggestions(
                beforeInput: target.documentContextBeforeInput,
                field: field,
                autocorrectionType: target.autocorrectionType,
                predictionModel: letterOdds,
                capitalizesNext: AutoCapitalization.shouldCapitalize(
                    mode: autoCapitalizationMode,
                    before: target.documentContextBeforeInput,
                    hasText: target.hasText
                )
            )
        }
        if next != suggestions {
            suggestions = next
            typingLog?.suggestionsChanged(next)
            onChange?()
        }
    }

    func acceptSuggestion(_ suggestion: KeyboardSuggestion) {
        // A queued tap on a prediction must not replace a word started in the meantime.
        if suggestion.kind == .nextWord,
           AutocorrectEngine.currentToken(in: target.documentContextBeforeInput) != nil
            || target.documentContextBeforeInput.map({ !($0.isEmpty || $0.last == " " || $0.last!.isNewline) }) == true
            || !suggestions.contains(suggestion) { return }
        // The strip shows a swipe's readings: mid-swipe they're only shown, after it one
        // takes the swiped word's place.
        if isSwiping { return }
        if suggestion.kind == .swiped {
            replaceSwipedWord(with: suggestion.replacement)
            return
        }
        // What the suggestion takes the place of: the word being typed, or a text
        // replacement's shortcut, which may hold digits or punctuation ("ad2").
        let before = target.documentContextBeforeInput
        let replacing = autocorrect.textReplacement(before: before)?.typed ?? AutocorrectEngine.currentToken(in: before)
        if let log = typingLog {
            let token = replacing ?? ""
            log.edit("suggestion", [
                TypingLogField("slot", suggestions.firstIndex(of: suggestion).map { .int($0) } ?? .null),
                TypingLogField("text", .string(suggestion.displayText)),
                TypingLogField("deleted", .string(token)),
                TypingLogField("inserted", .string(suggestion.replacement + " ")),
                TypingLogField("candidates", TypingLogger.candidates(suggestions)),
            ])
        }
        if let journal = learning {
            let token = replacing
            if let token, !token.isEmpty, token != suggestion.replacement {
                journal.record(.picked, typed: token, final: suggestion.replacement)
            } else {
                journal.didFinish(word: token)
                // Tapping what you typed over the autocorrection space would apply turns it down.
                if suggestion.kind == .keepLiteral, let token,
                   let autoFix = suggestions.first(where: \.willAutoReplace) {
                    journal.record(.keptTyped, typed: autoFix.replacement, final: token)
                }
            }
        }
        if let token = replacing {
            adaptiveLearning?.picked(
                token: token, replacement: suggestion.replacement,
                overCorrection: suggestion.kind == .keepLiteral && suggestions.contains(where: \.willAutoReplace),
                before: target.documentContextBeforeInput
            )
            for _ in 0..<token.count {
                target.deleteBackward()
            }
            if suggestion.kind == .keepLiteral {
                autocorrect.suppressAndKeep(token)
            }
        }
        autocorrect.clearPendingUndo()
        insert(suggestion.replacement + " ", source: nil)
        suggestionSpaceBefore = target.documentContextBeforeInput
        typingLog?.context("suggestion")
    }

    /// Types an emoji from the emoji panel: no spacing or autocorrect around it.
    func insertEmoji(_ text: String, source: String) {
        autocorrect.clearPendingUndo()
        adaptiveTouch?.forget()
        learning?.didFinish(word: AutocorrectEngine.currentToken(in: target.documentContextBeforeInput))
        insert(text, source: source)
    }

    // MARK: - Touches

    /// The key a touch going down at `point` presses: the one under it, or a neighbouring
    /// letter within `letterReach` points that is enough likelier to come next in the word.
    func key(at point: CGPoint) -> KeyboardKey? {
        let point = pointOnKeys(point)
        guard let key = layout.hitTest(point) else { return nil }
        guard key.character != nil, let model = letterOdds, layerState.layer == .letters,
              field == .text, autocorrect.isEnabled, target.autocorrectionType != .no,
              shift.state != .locked, model.language == language.rawValue else { return key }
        let reach = letterReach
        let near = layout.keys.filter {
            $0.character != nil && $0.kind != key.kind && Self.distance(from: point, to: $0.hitFrame) < reach
        }
        guard !near.isEmpty, let continuations = continuationMass(model) else { return key }
        // An accented letter without a key of its own is typed from its base letter's (é from e).
        var mass = [Double](repeating: 0, count: continuations.count)
        let keyed = Set(layout.keys.compactMap { key in
            key.character.flatMap { $0.count == 1 ? model.codes[Character($0)] : nil }
        })
        for (code, m) in continuations.enumerated() where m > 0 {
            mass[keyed.contains(UInt8(code)) ? code : Int(model.baseCodes[code])] += m
        }
        let total = mass.reduce(0, +)
        func odds(_ k: KeyboardKey) -> Double? {
            guard let c = k.character, c.count == 1, let code = model.codes[Character(c)] else { return nil }
            return log(mass[Int(code)] + total * 1e-4)
        }
        guard let own = odds(key) else { return key }
        var best = key
        var bestScore = own
        for other in near {
            guard let theirs = odds(other) else { continue }
            let score = theirs - letterOddsPerPoint * Double(Self.distance(from: point, to: other.hitFrame))
            if score > bestScore {
                best = other
                bestScore = score
            }
        }
        return best
    }

    /// The word list's next-letter frequencies after the word being typed, or nil when they
    /// can't help: inside something that isn't a word, or a word the list doesn't have.
    private func continuationMass(_ model: TypingDecoderModel) -> [Double]? {
        let before = target.documentContextBeforeInput ?? ""
        let token = AutocorrectEngine.currentToken(in: before)
        if token == nil, let last = before.last, last.isLetter || last.isNumber || last == "'" { return nil }
        guard let prefix = model.encode((token ?? "").lowercased()) else { return nil }
        if prefix.isEmpty {
            if let cached = wordStartMass { return cached }
            let mass = model.continuations(after: [])
            wordStartMass = .some(mass)
            return mass
        }
        if let cache = continuationCache, cache.prefix == prefix { return cache.mass }
        let mass = model.continuations(after: prefix)
        continuationCache = (prefix, mass)
        return mass
    }

    /// Whether a finger going down at `point`, in the strip above the keys, belongs to the
    /// keys: in the strip's lower half while you type (a key is down, or one went down or came
    /// up within `stripTouchGuard`) it is a row-1 key aimed high, not a pick. Nobody reads a
    /// new suggestion and taps it that fast; Record typing logs showed such picks were the e
    /// below the left slot, which kept "th" and typed a space where "the" was meant.
    func takesStripTouch(at point: CGPoint) -> Bool {
        let strip = layout.stripFrame
        guard point.y < layout.touchAreaFrame.minY, point.y >= strip.midY,
              point.x >= strip.minX, point.x < strip.maxX else { return false }
        if !touches.isEmpty { return true }
        guard let last = lastKeyTouchAt else { return false }
        let elapsed = clock() - last
        return elapsed >= 0 && elapsed < Self.stripTouchGuard
    }

    /// `point`, or for one above the keys (a strip touch `takesStripTouch(at:)` gave them)
    /// the same spot on row 1's top edge.
    private func pointOnKeys(_ point: CGPoint) -> CGPoint {
        let top = layout.touchAreaFrame.minY
        return point.y < top ? CGPoint(x: point.x, y: top) : point
    }

    private static func distance(from point: CGPoint, to rect: CGRect) -> CGFloat {
        let dx = max(rect.minX - point.x, 0, point.x - rect.maxX)
        let dy = max(rect.minY - point.y, 0, point.y - rect.maxY)
        return (dx * dx + dy * dy).squareRoot()
    }

    /// The kind of key a finger going down at `point` lands on, or nil off the keys: what
    /// `touchBegan(id:at:)` will press, up to the next-letter odds choosing a neighbouring
    /// letter. Cheap and free of side effects, so the key haptic can fire before any typing.
    func keyKind(at point: CGPoint) -> KeyKind? {
        layout.hitTest(pointOnKeys(point))?.kind
    }

    func touchBegan(id: Int, at point: CGPoint) {
        let downPoint = point
        let point = pointOnKeys(point)
        guard let key = key(at: point) else { return }
        lastKeyTouchAt = clock()
        // Rollover: a new key going down commits any character or accent picker still held.
        for other in touchOrder {
            guard var touch = touches[other], touch.key.character != nil, !touch.committed else { continue }
            if activeAccentTouchID == other, let menu = activeAccentMenu {
                typingLog?.ui("accent_pick", [
                    TypingLogField("id", .int(other)),
                    TypingLogField("index", .int(menu.selectedIndex)),
                    TypingLogField("chosen", .string(menu.selectedOption)),
                    TypingLogField("rollover", .bool(true)),
                ])
                commitAccentSelection(menu.selectedOption)
                activeAccentMenu = nil
                activeAccentTouchID = nil
            } else {
                commitCharacter(touch.key, at: touch.startPoint, down: touch.downPoint)
            }
            touch.committed = true
            touches[other] = touch
        }

        var touch = ActiveTouch(
            key: key,
            startPoint: point,
            point: point,
            committed: false,
            returnLayer: nil
        )
        touch.claimed = !key.hitFrame.contains(point)
        touch.downPoint = downPoint
        if key.character != nil {
            learning?.recordTap(on: key, at: point)
        }
        switch key.kind {
        case .shift:
            shift.shiftDown(now: clock())
            logState("shift_key")
        case .delete:
            deleteBackward(allowAutocorrectUndo: true)
        case .layerSwitch(let next):
            touch.returnLayer = layerState.layer
            layerState.switchTo(next)
            if next == .letters { shift.reset() }
            rebuildLayout(cause: "layer_key")
            if next == .letters { refreshAutoCapitalization() }
        case .character, .space, .returnKey, .hex, .globe, .emoji:
            break
        }
        touches[id] = touch
        touchOrder.append(id)
        onChange?()
    }

    @discardableResult
    func openAccentMenuIfEligible(forTouch id: Int) -> Bool {
        guard activeAccentMenu == nil,
              let touch = touches[id],
              !touch.committed,
              let base = touch.key.character else { return false }
        let dx = touch.point.x - touch.startPoint.x
        let dy = touch.point.y - touch.startPoint.y
        guard (dx * dx + dy * dy).squareRoot() <= Self.accentSlop else { return false }
        guard let options = AccentCatalog.options(for: base, uppercase: isUppercase, language: layout.language, letters: layout.letters),
              options.count > 1 else { return false }
        if let arrangement = AccentCatalog.arrangement(for: base, uppercase: isUppercase, language: layout.language, letters: layout.letters) {
            activeAccentMenu = AccentMenu.build(
                for: touch.key.frame,
                arrangement: arrangement,
                in: layout.width,
                horizontalInset: layout.horizontalInset
            )
        } else {
            activeAccentMenu = AccentMenu.build(
                for: touch.key.frame,
                rawOptions: options,
                in: layout.width,
                horizontalInset: layout.horizontalInset
            )
        }
        activeAccentTouchID = id
        typingLog?.ui("accent_open", [
            TypingLogField("id", .int(id)),
            TypingLogField("key", .string(TypingLogger.keyID(touch.key.kind))),
            TypingLogField("options", .strings(activeAccentMenu?.options ?? [])),
        ])
        onChange?()
        return true
    }

    func touchMoved(id: Int, to point: CGPoint) {
        guard var touch = touches[id] else { return }
        touch.point = point

        // 1. Active long-press accent picker: horizontal drag selects the alternate character.
        if activeAccentTouchID == id, var menu = activeAccentMenu {
            let nextIndex = menu.index(for: point)
            touches[id] = touch
            if nextIndex != menu.selectedIndex {
                menu.selectedIndex = nextIndex
                activeAccentMenu = menu
                onSelectionStep?()
                onChange?()
            }
            return
        }

        // 2. Spacebar cursor trackpad: horizontal drag >= 10 pt slides the text caret. With two
        // or more languages only once the space bar was held (`holdSpace`): a slide before
        // that is a language swipe.
        if touch.key.kind == .space {
            if touch.isLanguageSwipe {
                touches[id] = touch
                return
            }
            if !touch.isTrackpad, abs(point.x - touch.startPoint.x) >= Self.trackpadActivationDistance {
                if switchesLanguage {
                    touch.isLanguageSwipe = true
                    touches[id] = touch
                    return
                }
                startTrackpad(&touch, id: id, anchorX: point.x, hold: false)
                return
            }
            if touch.isTrackpad {
                let delta = point.x - touch.trackpadAnchorX
                let rawSteps = Int((delta / Self.trackpadPointsPerCharacter).rounded(.towardZero))
                if rawSteps != 0 {
                    touch.trackpadAnchorX += CGFloat(rawSteps) * Self.trackpadPointsPerCharacter
                    touch.trackpadOffset += rawSteps
                    touches[id] = touch
                    typingLog?.edit("cursor", [TypingLogField("offset", .int(rawSteps))])
                    tokenTaps.removeAll()
                    finishedWord = nil
                    adaptiveTouch?.forget()
                    suggestionSpaceBefore = nil
                    target.adjustTextPosition(byCharacterOffset: rawSteps)
                    onSelectionStep?()
                    refreshAutoCapitalization()
                    refreshSuggestions()
                    return
                }
                touches[id] = touch
                return
            }
        }

        // 3. Holding 123, #+= or shift and sliding: the character key under the finger shows its
        // pop-up; over any other key, or off the keys, none does. Never a swipe: swipes start on
        // letters only.
        if Self.slidesToType(touch) {
            let next = slideTarget(for: touch, at: point)
            if next?.kind != touch.slideKey?.kind {
                touch.slideKey = next
                if next != nil { touch.hasSlid = true }
                touches[id] = touch
                onChange?()
                return
            }
            touches[id] = touch
            return
        }

        // 4. Sliding across character keys updates the active key and re-anchors the hold timer.
        // A letter the next-letter odds claimed keeps the touch while it stays within reach.
        if touch.key.character != nil, !touch.committed,
           let under = layout.hitTest(point), under.character != nil, under.kind != touch.key.kind,
           !(touch.claimed && Self.distance(from: point, to: touch.key.hitFrame) < letterReach) {
            touch.claimed = false
            touch.key = under
            touch.startPoint = point
            touches[id] = touch
            onChange?()
            return
        }
        touches[id] = touch
    }

    /// The space bar held `trackpadHoldDelay` without sliding: the trackpad starts where the
    /// finger is, as Apple's does, so letting go types no space (a press held to move the
    /// cursor used to type one beside the space already there).
    func holdSpace(forTouch id: Int) {
        guard var touch = touches[id], touch.key.kind == .space, !touch.isTrackpad, !touch.isLanguageSwipe else { return }
        startTrackpad(&touch, id: id, anchorX: touch.point.x, hold: true)
    }

    private func startTrackpad(_ touch: inout ActiveTouch, id: Int, anchorX: CGFloat, hold: Bool) {
        touch.isTrackpad = true
        touch.trackpadAnchorX = anchorX
        lastSpaceAt = nil
        swipedWord = nil
        autocorrect.clearPendingUndo()
        touches[id] = touch
        var fields = [TypingLogField("id", .int(id))]
        if hold { fields.append(TypingLogField("hold", .bool(true))) }
        typingLog?.ui("trackpad_start", fields)
        onSelectionStep?()
        onChange?()
    }

    func touchEnded(id: Int, at point: CGPoint) {
        guard var touch = touches.removeValue(forKey: id) else { return }
        touchOrder.removeAll { $0 == id }
        lastKeyTouchAt = clock()
        touch.point = point

        if activeAccentTouchID == id, let menu = activeAccentMenu {
            let index = menu.index(for: point)
            let chosen = menu.options[index]
            activeAccentMenu = nil
            activeAccentTouchID = nil
            // Let go well below the held key, the callout closes with nothing typed, as Apple's
            // does (iOS 27.2: 55 pt down types, 60 pt cancels; any distance up types).
            if point.y - menu.keyFrame.midY > Self.accentCancelDrop {
                typingLog?.ui("accent_cancel", [TypingLogField("id", .int(id))])
                onChange?()
                return
            }
            if !touch.committed {
                typingLog?.ui("accent_pick", [
                    TypingLogField("id", .int(id)),
                    TypingLogField("index", .int(index)),
                    TypingLogField("chosen", .string(chosen)),
                ])
                commitAccentSelection(chosen)
            }
            onChange?()
            return
        }

        switch touch.key.kind {
        case .character:
            if !touch.committed {
                commitCharacter(touch.key, at: touch.startPoint, down: touch.downPoint)
            }
        case .shift:
            // Hold shift, slide to a letter, let go: a capital, as on Apple's keyboard.
            if let under = slideTarget(for: touch, at: point) {
                commitCharacter(under, at: point)
            }
            shift.shiftUp()
            logState("shift_key")
        case .space:
            if touch.isTrackpad {
                typingLog?.ui("trackpad_end", [
                    TypingLogField("id", .int(id)),
                    TypingLogField("offset", .int(touch.trackpadOffset)),
                ])
                typingLog?.context("trackpad")
            } else if touch.isLanguageSwipe, let step = Self.languageSwipeStep(from: touch.startPoint, to: point) {
                typingLog?.ui("language_swipe", [
                    TypingLogField("id", .int(id)),
                    TypingLogField("step", .int(step)),
                ])
                onLanguageSwipe?(step)
            } else {
                typeSpace(at: CGPoint(x: layout.unsplitX(touch.startPoint.x), y: touch.startPoint.y))
            }
        case .returnKey:
            if target.enablesReturnKeyAutomatically == true && !target.hasText {
                typingLog?.edit("return", [
                    TypingLogField("returnKeyType", .string(TypingLogTraits.name(target.returnKeyType ?? .default))),
                    TypingLogField("ignored", .bool(true)),
                ])
                break
            }
            learning?.didFinish(word: AutocorrectEngine.currentToken(in: target.documentContextBeforeInput))
            autocorrect.clearPendingUndo()
            typingLog?.edit("return", [
                TypingLogField("inserted", .string("\n")),
                TypingLogField("returnKeyType", .string(TypingLogTraits.name(target.returnKeyType ?? .default))),
                TypingLogField("send", .bool(target.returnKeyType == .send)),
            ])
            onReturn?()
            // A text replacement goes in on return too, as on Apple's keyboard; the new line
            // goes in on its own, so a field's return (Send, Search) still sees just "\n".
            _ = applyTextReplacement(ending: "")
            insert("\n", source: nil)
            typingLog?.context("return")
        case .layerSwitch:
            // Hold 123, slide to a key, let go: type it and go back. Back on the page key after
            // a slide: nothing, and back too. Off the keys: nothing, and the page stays.
            if let returnLayer = touch.returnLayer {
                if let under = slideTarget(for: touch, at: point), let character = under.character {
                    if !correctWord(endingWith: character) {
                        autocorrect.clearPendingUndo()
                        insert(character, source: "slide")
                    }
                    layerState.switchTo(returnLayer)
                    rebuildLayout(cause: "layer_slide")
                    refreshAutoCapitalization()
                } else if touch.hasSlid, layout.hitTest(point)?.kind == .layerSwitch(returnLayer) {
                    layerState.switchTo(returnLayer)
                    if returnLayer == .letters { shift.reset() }
                    rebuildLayout(cause: "layer_slide_back")
                    if returnLayer == .letters { refreshAutoCapitalization() }
                }
            }
        case .emoji:
            onEmojiKey?()
        case .delete, .hex, .globe:
            break
        }
        onChange?()
    }

    /// A press that types the character key it slides onto: 123, ABC and #+= (with a page to
    /// go back to), and shift.
    private static func slidesToType(_ touch: ActiveTouch) -> Bool {
        touch.returnLayer != nil || touch.key.kind == .shift
    }

    /// The character key under `point` that such a press types on letting go, or nil over
    /// any other key or off the keys.
    private func slideTarget(for touch: ActiveTouch, at point: CGPoint) -> KeyboardKey? {
        guard Self.slidesToType(touch), let under = layout.hitTest(point),
              under.character != nil, under.kind != touch.key.kind else { return nil }
        return under
    }

    /// A language swipe's direction, or nil when it was too short or more up than along.
    private static func languageSwipeStep(from start: CGPoint, to end: CGPoint) -> Int? {
        let dx = end.x - start.x
        guard abs(dx) >= languageSwipeDistance, abs(dx) >= abs(end.y - start.y) else { return nil }
        return dx < 0 ? 1 : -1
    }

    func touchCancelled(id: Int) {
        if activeAccentTouchID == id {
            activeAccentMenu = nil
            activeAccentTouchID = nil
            typingLog?.ui("accent_cancel", [TypingLogField("id", .int(id))])
        }
        guard touches.removeValue(forKey: id) != nil else { return }
        touchOrder.removeAll { $0 == id }
        onChange?()
    }

    func cancelAllTouches() {
        let hadTouches = !touches.isEmpty || activeAccentMenu != nil || isSwiping
        touches.removeAll()
        touchOrder.removeAll()
        isSwiping = false
        swipePreview = nil
        activeAccentMenu = nil
        activeAccentTouchID = nil
        if shift.isHeld {
            shift.shiftUp()
            logState("cancel")
        }
        if hadTouches { onChange?() }
    }

    // MARK: - Delete

    /// The delete key repeats after 360 ms, accelerates at 850 ms, and removes whole words after 1.4 s.
    static let repeatDelay: TimeInterval = 0.36
    static let repeatInterval: TimeInterval = 0.075
    static let fastRepeatAfter: TimeInterval = 0.85
    static let wordDeleteAfter: TimeInterval = 1.4

    func repeatDelete(heldFor: TimeInterval) {
        autocorrect.clearPendingUndo()
        if heldFor >= Self.wordDeleteAfter {
            deleteWordBackward()
        } else if heldFor >= Self.fastRepeatAfter {
            deleteBackward(allowAutocorrectUndo: false, source: "repeat_fast")
            deleteBackward(allowAutocorrectUndo: false, source: "repeat_fast")
        } else {
            deleteBackward(allowAutocorrectUndo: false, source: "repeat")
        }
    }

    func deleteBackward(allowAutocorrectUndo: Bool = true, source: String = "key") {
        // Right after a swipe, delete takes back its whole word (and the space it added).
        let swiped = swipedWord
        swipedWord = nil
        if allowAutocorrectUndo, let swiped, deleteSwipedWord(swiped) { return }
        if allowAutocorrectUndo,
           let undo = autocorrect.consumePendingUndo(),
           let before = target.documentContextBeforeInput,
           before.hasSuffix(undo.insertedText) {
            typingLog?.edit("autocorrect_undo", [
                TypingLogField("deleted", .string(undo.insertedText)),
                TypingLogField("inserted", .string(undo.original)),
            ])
            for _ in 0..<undo.insertedText.count {
                target.deleteBackward()
            }
            tokenTaps.removeAll()
            finishedWord = nil
            adaptiveLearning?.undone(original: undo.original)
            target.insertText(undo.original)
            for change in undo.changes {
                autocorrect.suppressAndKeep(change.from)
                learning?.record(.undone, typed: change.to, final: change.from)
            }
            lastSpaceAt = nil
            didEdit()
            return
        }
        autocorrect.clearPendingUndo()
        learning?.willErase(word: AutocorrectEngine.currentToken(in: target.documentContextBeforeInput))
        adaptiveLearning?.willDelete(before: target.documentContextBeforeInput)
        if let log = typingLog {
            let deleted = target.documentContextBeforeInput?.last.map { String($0) }
            log.edit("delete", [
                deleted.map { TypingLogField("deleted", .string($0)) } ?? TypingLogField("deletedCount", .int(1)),
                TypingLogField("src", .string(source)),
            ])
        }
        if let last = target.documentContextBeforeInput?.last, last.isLetter || last == "'", !tokenTaps.isEmpty {
            tokenTaps.removeLast()
        } else {
            tokenTaps.removeAll()
        }
        target.deleteBackward()
        lastSpaceAt = nil
        didEdit()
    }

    func deleteWordBackward() {
        swipedWord = nil
        autocorrect.clearPendingUndo()
        adaptiveTouch?.forget()
        guard let before = target.documentContextBeforeInput, !before.isEmpty else {
            deleteBackward(allowAutocorrectUndo: false, source: "repeat_word")
            return
        }
        learning?.willErase(word: AutocorrectEngine.currentToken(in: before))
        var count = 0
        var index = before.endIndex
        while index > before.startIndex, before[before.index(before: index)].isWhitespace {
            index = before.index(before: index)
            count += 1
        }
        while index > before.startIndex, !before[before.index(before: index)].isWhitespace {
            index = before.index(before: index)
            count += 1
        }
        tokenTaps.removeAll()
        typingLog?.edit("delete_word", [
            TypingLogField("deleted", .string(String(before.suffix(max(1, count))))),
            TypingLogField("src", .string("repeat_word")),
        ])
        for _ in 0..<max(1, count) {
            target.deleteBackward()
        }
        lastSpaceAt = nil
        didEdit()
        typingLog?.context("delete_word")
    }

    // MARK: - Typing

    /// `down`: where a tap's finger went down, for Adaptive touch (nil for a slide from shift).
    private func commitCharacter(_ key: KeyboardKey, at point: CGPoint, down: CGPoint? = nil) {
        guard let character = key.character else { return }
        spaceAfterSwipedWord(before: character)
        let text = layerState.layer == .letters && shift.isUppercase ? character.uppercased() : character
        if layerState.layer == .letters {
            shift.didType()
        }
        if !correctWord(endingWith: text) {
            autocorrect.clearPendingUndo()
            insert(text, source: "key", at: point)
        }
        if let down, layerState.layer == .letters, character.first?.isLetter == true {
            adaptiveLearning?.typed(character, down: down, slid: pointOnKeys(down) != point)
        }
        logState("typed")
    }

    /// Punctuation that closes a word, as Apple's keyboard treats it: , . ! ? ; : ) and ".
    static let wordEnders: Set<Character> = [",", ".", "!", "?", ";", ":", ")", "\""]

    /// Punctuation right after a word corrects it as space does ("teh," becomes "the,"), and
    /// goes in after the fix. Backspace right after takes back both, as after a space ("teh").
    /// False when there is nothing to fix: the caller types the mark as usual.
    private func correctWord(endingWith text: String) -> Bool {
        guard text.count == 1, let mark = text.first, Self.wordEnders.contains(mark), swipedWord == nil else { return false }
        if applyTextReplacement(ending: text) { return true }
        let before = target.documentContextBeforeInput
        guard let last = before?.last, last.isLetter, let token = AutocorrectEngine.currentToken(in: before) else { return false }
        // "approx." and "etc." are abbreviations, not typos.
        if mark == ".", AutocorrectEngine.abbreviations.contains(token.lowercased()) { return false }
        learning?.didFinish(word: token)
        syncLearning()
        autocorrect.decoderTaps = currentTokenTaps
        guard finishWord(ending: text) else { return false }
        finishedWord = nil
        lastSpaceAt = nil
        layerState.didInsert(text)
        rebuildLayoutIfNeeded()
        didEdit()
        return true
    }

    /// Puts your text replacement in place of its shortcut as `ending` (a space or a closing
    /// mark; nothing for return, whose new line follows) finishes it, as Apple's keyboard does
    /// (black box, iOS 27.2), whatever autocorrect would do. There, delete right after takes
    /// only `ending` back, never the replacement, and keeping the shortcut never turns it
    /// off: so no undo is recorded and nothing is learned. False, typing nothing, when no
    /// shortcut ends the text.
    private func applyTextReplacement(ending: String) -> Bool {
        let before = target.documentContextBeforeInput
        guard field == .text, target.autocorrectionType != .no,
              let replaced = autocorrect.textReplacement(before: before) else { return false }
        adaptiveLearning?.finish(token: replaced.typed, corrected: replaced.replacement, before: before)
        for _ in 0..<replaced.typed.count {
            target.deleteBackward()
        }
        typingLog?.edit("autocorrect", [
            TypingLogField("from", .string(replaced.typed)),
            TypingLogField("to", .string(replaced.replacement)),
            TypingLogField("deleted", .string(replaced.typed)),
            TypingLogField("inserted", .string(replaced.replacement + ending)),
            TypingLogField("textReplacement", .bool(true)),
        ])
        target.insertText(replaced.replacement + ending)
        autocorrect.clearPendingUndo()
        tokenTaps.removeAll()
        swipedWord = nil
        finishedWord = nil
        lastSpaceAt = nil
        layerState.didInsert(ending)
        rebuildLayoutIfNeeded()
        didEdit()
        return true
    }

    /// The word a space finished, while the next one is typed: that word's space or mark may
    /// still revise it ("well see" becomes "we'll see"), as Apple's keyboard decides some
    /// words at the next one.
    private struct FinishedWord {
        /// The word as it reads now.
        var text: String
        /// As you typed it: one you gave an apostrophe yourself is never revised.
        var typed: String
        /// The text before the cursor right after its space.
        var before: String
        /// Where its letters were tapped, and its space: a space near the letter row's edge
        /// may have been a letter, and the next word joins it (`AutocorrectEngine.merge`).
        var taps: [CGPoint]?
        var space: CGPoint?
    }

    private var finishedWord: FinishedWord?

    /// Corrects the word before the cursor as `ending` (a space or a closing mark) finishes
    /// it, and revises the word before it now that this one follows (`AutocorrectEngine.
    /// revision`); then types `ending`. One backspace right after puts back what was typed.
    /// False, typing nothing, when neither changes.
    private func finishWord(ending: String) -> Bool {
        let before = target.documentContextBeforeInput ?? ""
        guard let token = AutocorrectEngine.currentToken(in: before) else { return false }
        if let finished = finishedWord, before == finished.before + token,
           let taps = finished.taps, let space = finished.space, field == .text, target.autocorrectionType != .no,
           let merged = autocorrect.merge(
               first: finished.typed, firstTaps: taps, space: space, second: token,
               before: String(finished.before.dropLast())
           ) {
            // Two words machine-joined teach nothing.
            adaptiveTouch?.forget()
            let original = finished.text + " " + token
            for _ in 0..<original.count {
                target.deleteBackward()
            }
            tokenTaps.removeAll()
            typingLog?.edit("autocorrect", [
                TypingLogField("from", .string(original)),
                TypingLogField("to", .string(merged)),
                TypingLogField("deleted", .string(original)),
                TypingLogField("inserted", .string(merged + ending)),
                TypingLogField("merged", .bool(true)),
            ])
            target.insertText(merged + ending)
            learning?.record(.autocorrected, typed: token, final: merged)
            autocorrect.recordAppliedCorrection(
                original: original, insertedText: merged + ending, changes: [.init(from: token, to: merged)]
            )
            swipedWord = nil
            return true
        }
        let fix = autocorrect.replacementOnSpace(beforeInput: before, field: field, autocorrectionType: target.autocorrectionType)
        let word = fix?.replacement ?? token
        var revised: (from: String, to: String)?
        if let finished = finishedWord, before == finished.before + token, !finished.typed.contains("'"),
           let to = autocorrect.revision(
               of: finished.text, before: String(finished.before.dropLast()), next: word,
               field: field, autocorrectionType: target.autocorrectionType
           ) {
            revised = (finished.text, to)
        }
        guard fix != nil || revised != nil else { return false }
        adaptiveLearning?.finish(token: token, corrected: fix?.replacement, previousRevised: revised != nil, before: before)
        let original = (revised.map { $0.from + " " } ?? "") + token
        let inserted = (revised.map { $0.to + " " } ?? "") + word + ending
        for _ in 0..<original.count {
            target.deleteBackward()
        }
        tokenTaps.removeAll()
        var fields = [
            TypingLogField("from", .string(token)),
            TypingLogField("to", .string(word)),
            TypingLogField("deleted", .string(original)),
            TypingLogField("inserted", .string(inserted)),
        ]
        if ending != " " { fields.append(TypingLogField("ending", .string(ending))) }
        if let revised { fields.append(TypingLogField("revised", .strings([revised.from, revised.to]))) }
        typingLog?.edit("autocorrect", fields)
        target.insertText(inserted)
        var changes: [AutocorrectEngine.AppliedCorrection.Change] = []
        if let revised {
            changes.append(.init(from: revised.from, to: revised.to))
            learning?.record(.autocorrected, typed: revised.from, final: revised.to)
        }
        if fix != nil {
            changes.append(.init(from: token, to: word))
            learning?.record(.autocorrected, typed: token, final: word)
        }
        autocorrect.recordAppliedCorrection(original: original, insertedText: inserted, changes: changes)
        swipedWord = nil
        return true
    }

    /// The taps of the word being typed when every letter came from a key, else nil.
    private var currentTokenTaps: [CGPoint]? {
        let taps = tokenTaps.compactMap { $0 }
        return taps.count == tokenTaps.count && !taps.isEmpty ? taps : nil
    }

    private func commitAccentSelection(_ chosen: String) {
        spaceAfterSwipedWord(before: chosen)
        if layerState.layer == .letters {
            shift.didType()
        }
        autocorrect.clearPendingUndo()
        insert(chosen, source: "accent")
        logState("typed")
    }

    /// `point` is where the space bar was tapped, as on the unsplit keyboard.
    private func typeSpace(at point: CGPoint? = nil) {
        // Space never autocorrects a swiped word: it is a reading, not a typo.
        let afterSwipe = swipedWord != nil
        swipedWord = nil
        let now = clock()
        if let before = suggestionSpaceBefore {
            suggestionSpaceBefore = nil
            if before.last == " ", before == target.documentContextBeforeInput {
                // The suggestion typed this space already. It still counts as one, so a quick
                // second press types ". ".
                typingLog?.edit("space_swallowed", [TypingLogField("after", .string("suggestion"))])
                lastSpaceAt = now
                tokenTaps.removeAll()
                layerState.didInsert(" ")
                rebuildLayoutIfNeeded()
                onChange?()
                return
            }
        }
        if DoubleSpacePeriod.shouldReplace(before: target.documentContextBeforeInput, lastSpaceAt: lastSpaceAt, now: now) {
            autocorrect.clearPendingUndo()
            typingLog?.edit("double_space_period", [
                TypingLogField("deleted", .string(" ")),
                TypingLogField("inserted", .string(". ")),
            ])
            target.deleteBackward()
            target.insertText(". ")
            tokenTaps.removeAll()
            finishedWord = nil
            lastSpaceAt = nil
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
            didEdit()
            typingLog?.context("space")
            return
        }
        let token = AutocorrectEngine.currentToken(in: target.documentContextBeforeInput)
        let taps = currentTokenTaps
        learning?.didFinish(word: token)
        syncLearning()
        autocorrect.decoderTaps = taps
        if !afterSwipe, applyTextReplacement(ending: " ") {
            lastSpaceAt = now
            typingLog?.context("space")
            return
        }
        if !afterSwipe, finishWord(ending: " ") {
            rememberFinishedWord(typed: token, taps: taps, space: point)
            lastSpaceAt = now
            layerState.didInsert(" ")
            rebuildLayoutIfNeeded()
            didEdit()
            typingLog?.context("space")
            return
        }
        autocorrect.clearPendingUndo()
        insert(" ", source: "key")
        if !afterSwipe, let token { rememberFinishedWord(typed: token, taps: taps, space: point) }
        lastSpaceAt = now
        typingLog?.context("space")
    }

    /// The word right before the space just typed, for the next word's revision.
    private func rememberFinishedWord(typed: String?, taps: [CGPoint]?, space: CGPoint?) {
        guard let typed, let before = target.documentContextBeforeInput, before.last == " ",
              let word = AutocorrectEngine.currentToken(in: String(before.dropLast())) else {
            finishedWord = nil
            return
        }
        // After a split or a merge the word reads other letters than you typed ("of the" for
        // "ofthe"): it is remembered as it reads, without taps.
        guard word.filter({ $0 != "'" }).count == typed.count else {
            finishedWord = FinishedWord(text: word, typed: word, before: before, taps: nil, space: nil)
            return
        }
        finishedWord = FinishedWord(text: word, typed: typed, before: before, taps: taps, space: space)
    }

    /// `source` names the edit in typing logs; nil when the caller writes its own line.
    private func insert(_ text: String, source: String?, at point: CGPoint? = nil) {
        swipedWord = nil
        // Anything but the next word's letters ends the chance to revise the word before.
        if !(text.count == 1 && (text.first?.isLetter == true || text == "'")) {
            finishedWord = nil
        }
        if text.count == 1, text.first?.isLetter == true {
            // Spatial autocorrect reads taps as on the unsplit keyboard (`KeyboardLayout.unsplitX`).
            tokenTaps.append(point.map { CGPoint(x: layout.unsplitX($0.x), y: $0.y) })
        } else {
            tokenTaps.removeAll()
            if let adaptive = adaptiveLearning {
                let before = target.documentContextBeforeInput
                adaptive.finish(token: AutocorrectEngine.currentToken(in: before), before: before)
            }
        }
        if let source {
            typingLog?.edit("insert", [
                TypingLogField("inserted", .string(text)),
                TypingLogField("src", .string(source)),
            ])
        }
        target.insertText(text)
        lastSpaceAt = nil
        layerState.didInsert(text)
        rebuildLayoutIfNeeded()
        didEdit()
    }

    /// Hands what the journal learned (typos, rejected corrections) to autocorrect;
    /// nothing while Learn how I type is off.
    private func syncLearning() {
        autocorrect.applyLearning(journal?.typingLearning ?? TypingLearning())
    }

    private func didEdit() {
        suggestionSpaceBefore = nil
        refreshAutoCapitalization()
        refreshSuggestions()
        onEdit?()
    }

    // MARK: - Swipe typing (Settings › Keyboard › Swipe typing; SwipeTyping.swift)

    private(set) var swipedWord: SwipedWord?
    /// From a touch becoming a swipe until it ends.
    private(set) var isSwiping = false
    /// What the swipe under way reads as so far, for the strip.
    private(set) var swipePreview: String?

    /// A letter touch became a swipe: its key no longer types.
    func beginSwipe(touchID id: Int) {
        guard touches.removeValue(forKey: id) != nil else { return }
        touchOrder.removeAll { $0 == id }
        isSwiping = true
        swipePreview = nil
        typingLog?.ui("swipe_start", [TypingLogField("id", .int(id))])
        refreshSuggestions()
        onChange?()
    }

    /// The swipe under way reads as `words` so far (best first): the strip shows the first
    /// that may be offered.
    func showSwipePreview(_ words: [String]) {
        guard isSwiping else { return }
        let next = swipeReadings(words).first.map { swipeCased($0, capital: swipeCapital) }
        guard next != swipePreview else { return }
        swipePreview = next
        refreshSuggestions()
    }

    /// A swipe ended. `words` are its readings, best first (none when it was cancelled): the
    /// first that may be offered goes in, after a space unless the text before ends in one or
    /// in opening punctuation, as Apple's keyboard types a swiped word, with a capital when
    /// shift is on (all capitals with caps lock); the next three wait in the strip.
    func endSwipe(typing words: [String], points: Int, decodeTime: TimeInterval?) {
        guard isSwiping else { return }
        isSwiping = false
        swipePreview = nil
        // The finger came up off a key: a quick tap on the strip's lower half next is a key.
        lastKeyTouchAt = clock()
        let readings = swipeReadings(words)
        if let log = typingLog {
            log.ui("swipe_end", [
                TypingLogField("points", .int(points)),
                TypingLogField("ms", decodeTime.map { .ms($0 * 1000) } ?? .null),
                TypingLogField("readings", .strings(Array(readings.prefix(4)))),
            ])
        }
        guard let first = readings.first else {
            refreshSuggestions()
            onChange?()
            return
        }
        let before = target.documentContextBeforeInput
        let space = SwipeTyping.needsSpace(before: before)
        let capital = swipeCapital
        let word = swipeCased(first, capital: capital)
        let inserted = (space ? " " : "") + word
        if space { learning?.didFinish(word: AutocorrectEngine.currentToken(in: before)) }
        autocorrect.clearPendingUndo()
        tokenTaps.removeAll()
        adaptiveTouch?.forget()
        typingLog?.edit("insert", [
            TypingLogField("inserted", .string(inserted)),
            TypingLogField("src", .string("swipe")),
        ])
        target.insertText(inserted)
        finishedWord = nil
        lastSpaceAt = nil
        layerState.didInsert(inserted)
        if layerState.layer == .letters { shift.didType() }
        swipedWord = SwipedWord(
            inserted: inserted,
            word: word,
            alternatives: swipeAlternatives(readings, capital: capital)
        )
        logState("typed")
        didEdit()
        typingLog?.context("swipe")
    }

    /// Puts another of the swipe's readings in its word's place; the word joins the others.
    private func replaceSwipedWord(with text: String) {
        guard var swiped = swipedWord, let before = target.documentContextBeforeInput, before.hasSuffix(swiped.word) else {
            swipedWord = nil
            refreshSuggestions()
            return
        }
        typingLog?.edit("suggestion", [
            TypingLogField("slot", suggestions.firstIndex { $0.replacement == text }.map { .int($0) } ?? .null),
            TypingLogField("text", .string(text)),
            TypingLogField("deleted", .string(swiped.word)),
            TypingLogField("inserted", .string(text)),
            TypingLogField("candidates", TypingLogger.candidates(suggestions)),
        ])
        for _ in 0..<swiped.word.count {
            target.deleteBackward()
        }
        target.insertText(text)
        swiped.inserted = String(swiped.inserted.dropLast(swiped.word.count)) + text
        swiped.alternatives = Array(([swiped.word] + swiped.alternatives.filter { $0 != text }).prefix(3))
        swiped.word = text
        swipedWord = swiped
        lastSpaceAt = nil
        didEdit()
        typingLog?.context("suggestion")
    }

    /// Right after a swipe, a tapped letter, digit, opening mark or symbol goes in after a
    /// space, as Apple's keyboard keeps one pending after a swiped word ("hello" then "w"
    /// types "hello w", "(" types "hello ("); punctuation in `swipeJoiners` goes against the
    /// word ("hello,"). Shift as you set it stays; with it off, the space may bring a capital
    /// (a field that capitalises every word).
    private func spaceAfterSwipedWord(before text: String) {
        guard let swiped = swipedWord, let first = text.first, !Self.swipeJoiners.contains(first),
              let before = target.documentContextBeforeInput, before.hasSuffix(swiped.word) else { return }
        swipedWord = nil
        learning?.didFinish(word: AutocorrectEngine.currentToken(in: before))
        typingLog?.edit("insert", [
            TypingLogField("inserted", .string(" ")),
            TypingLogField("src", .string("swipe_space")),
        ])
        target.insertText(" ")
        tokenTaps.removeAll()
        finishedWord = nil
        lastSpaceAt = nil
        if shift.state == .off { refreshAutoCapitalization() }
    }

    /// Takes back what a swipe typed when it is still right before the cursor.
    private func deleteSwipedWord(_ swiped: SwipedWord) -> Bool {
        guard let before = target.documentContextBeforeInput, before.hasSuffix(swiped.inserted) else { return false }
        autocorrect.clearPendingUndo()
        typingLog?.edit("delete", [
            TypingLogField("deleted", .string(swiped.inserted)),
            TypingLogField("src", .string("swipe")),
        ])
        for _ in 0..<swiped.inserted.count {
            target.deleteBackward()
        }
        tokenTaps.removeAll()
        lastSpaceAt = nil
        didEdit()
        return true
    }

    // MARK: - Layout

    private func rebuildLayoutIfNeeded() {
        if layout.layer != layerState.layer {
            rebuildLayout(cause: "layer_auto")
        }
    }

    private func rebuildLayout(cause: String = "layout") {
        layout = KeyboardLayout(
            width: width,
            orientation: orientation,
            layer: layerState.layer,
            field: field,
            showsGlobe: showsGlobe,
            showsEmoji: showsEmoji,
            horizontalInset: horizontalInset,
            language: layoutLanguage ?? language,
            letters: letterLayout,
            posture: posture,
            windowInset: windowInset,
            hexPlacement: hexPlacement,
            phoneSize: phoneSize
        )
        autocorrect.setKeys(layout.language, letters: layout.letters)
        syncDecoderKeys()
        logState(cause)
    }

    /// The letter keys Spatial autocorrect scores taps against: this layout's, moved by
    /// `tapOffsets`; with Adaptive touch on, by its personal centres instead (none while it
    /// has too little evidence), with the others beside them as the baseline no correction
    /// may go past (`AutocorrectEngine.decoderBaselineKeys`). Off the letters page the last
    /// letters page's stay: a closing mark from the 123 page still reads the word's taps
    /// ("ofthe." -> "of the.").
    private func syncDecoderKeys() {
        guard let keys = TypingDecoder.Keys(layout: layout, offsets: tapOffsets) else { return }
        guard let adaptive = adaptiveTouch?.offsets(for: layout),
              let personal = TypingDecoder.Keys(layout: layout, offsets: adaptive) else {
            autocorrect.decoderKeys = keys
            autocorrect.decoderBaselineKeys = nil
            return
        }
        autocorrect.decoderKeys = personal
        autocorrect.decoderBaselineKeys = personal == keys ? nil : keys
    }

    /// Hands the layout and shift to typing logs, which writes a line when either changed.
    func logState(_ cause: String) {
        typingLog?.keyboardState(layout: layout, shift: shift, cause: cause)
    }
}
